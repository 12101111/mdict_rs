use crate::*;
use async_trait::async_trait;
use log::info;
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteConnection, SqliteJournalMode, SqlitePool, SqlitePoolOptions,
};
use sqlx::{ConnectOptions, Connection, Executor};
use std::path::{Path, PathBuf};
use std::{
    fs::{self, OpenOptions},
    io,
};
use tokio::stream::StreamExt;

const DB_INIT: &str = include_str!("../migration/init.sql");

pub struct MDictSqliteBuilder {
    conn: SqliteConnection,
    index: MDictMemIndex,
}

struct MdxBlock {
    block_index: i32,
    block_offset: i64,
    block_size: i64,
}

struct MdxIndex {
    keyword: String,
    block_index: i32,
    record_offset: i32,
    record_size: i32,
}

#[derive(sqlx::FromRow, Debug)]
struct MdxQuery {
    keyword: String,
    block_index: i32,
    record_offset: i32,
    record_size: i32,
    block_offset: i64,
    block_size: i64,
}

struct MddBlock {
    file_index: i32,
    block_index: i32,
    block_offset: i64,
    block_size: i64,
}

struct MddIndex {
    keyword: String,
    file_index: i32,
    block_index: i32,
    record_offset: i32,
    record_size: i32,
}

#[derive(sqlx::FromRow, Debug)]
struct MddQuery {
    keyword: String,
    file_index: i32,
    block_index: i32,
    record_offset: i32,
    record_size: i32,
    block_offset: i64,
    block_size: i64,
}

impl MDictSqliteBuilder {
    async fn build(mut self) -> sqlx::Result<()> {
        self.conn.execute(DB_INIT).await?;
        self.write_mdx().await?;
        self.write_mdd().await?;
        let now = std::time::Instant::now();
        let mut transaction = self.conn.begin().await?;
        transaction
            .execute("CREATE UNIQUE INDEX mdx_key ON mdx_index (keyword)")
            .await?;
        transaction
            .execute("CREATE UNIQUE INDEX mdd_key ON mdd_index (keyword)")
            .await?;
        transaction
            .execute(
                sqlx::query("insert into meta (key, value) values ( ?1, ?2)")
                    .bind("version")
                    .bind(env!("CARGO_PKG_VERSION")),
            )
            .await?;
        transaction.commit().await?;
        info!("Build index cache in {:?}", now.elapsed());
        Ok(())
    }
    async fn write_mdx(&mut self) -> sqlx::Result<()> {
        let now = std::time::Instant::now();
        let mut transaction = self.conn.begin().await?;
        for (i, v) in self.index.mdx_block.iter().enumerate() {
            let mdx_block = MdxBlock {
                block_index: i as i32,
                block_offset: v.offset as i64,
                block_size: v.comp_size as i64,
            };
            sqlx::query!(
                r"
                    insert into mdx_block (block_index, block_offset, block_size)
                    values ( ?1, ?2, ?3 )
                ",
                mdx_block.block_index,
                mdx_block.block_offset,
                mdx_block.block_size
            )
            .execute(&mut transaction)
            .await?;
        }
        transaction.commit().await?;
        info!("Build mdx block index in {:?}", now.elapsed());
        let mut transaction = self.conn.begin().await?;
        let now = std::time::Instant::now();
        for (k, v) in self.index.mdx_index.iter() {
            let mdx_index = MdxIndex {
                keyword: String::from_utf8(k).unwrap(),
                block_index: v.block as i32,
                record_offset: v.offset as i32,
                record_size: v.len as i32,
            };
            sqlx::query!(
                r"
                    insert into mdx_index (keyword, block_index, record_offset, record_size)
                    values ( ?1, ?2, ?3 , ?4)
                ",
                mdx_index.keyword,
                mdx_index.block_index,
                mdx_index.record_offset,
                mdx_index.record_size
            )
            .execute(&mut transaction)
            .await?;
        }
        transaction.commit().await?;
        info!("Build mdx keyword index in {:?}", now.elapsed());
        Ok(())
    }
    async fn write_mdd(&mut self) -> sqlx::Result<()> {
        let now = std::time::Instant::now();
        let mut transaction = self.conn.begin().await?;
        for (i, file) in self.index.mdd_blocks.iter().enumerate() {
            for (j, block) in file.iter().enumerate() {
                let mdd_block = MddBlock {
                    file_index: i as i32,
                    block_index: j as i32,
                    block_offset: block.offset as i64,
                    block_size: block.comp_size as i64,
                };
                sqlx::query!(
                    r"
                        insert into mdd_block (file_index, block_index, block_offset, block_size)
                        values ( ?1, ?2, ?3, ?4 )
                    ",
                    mdd_block.file_index,
                    mdd_block.block_index,
                    mdd_block.block_offset,
                    mdd_block.block_size
                )
                .execute(&mut transaction)
                .await?;
            }
        }
        transaction.commit().await?;
        info!("Build mdd block index in {:?}", now.elapsed());
        let mut transaction = self.conn.begin().await?;
        let now = std::time::Instant::now();
        for (ref key, (file, index)) in self.index.mdd_index.iter() {
            let mdd_index = MddIndex {
                keyword: String::from_utf8(key.to_vec()).unwrap(),
                file_index: *file as i32,
                block_index: index.block as i32,
                record_offset: index.offset as i32,
                record_size: index.len as i32,
            };
            sqlx::query!(
                r"
                    insert into mdd_index (keyword, file_index, block_index, record_offset, record_size)
                    values ( ?1, ?2, ?3, ?4, ?5 )
                ",
                mdd_index.keyword,
                mdd_index.file_index,
                mdd_index.block_index,
                mdd_index.record_offset,
                mdd_index.record_size
            )
            .execute(&mut transaction)
            .await?;
        }
        transaction.commit().await?;
        info!("Build mdd keyword index in {:?}", now.elapsed());
        Ok(())
    }
}

async fn open_db(file: impl AsRef<Path>) -> Option<SqlitePool> {
    let db_file = file.as_ref().with_extension("db");
    if !db_file.exists() {
        info!("Index not exists");
        return None;
    }
    let options = SqliteConnectOptions::new()
        .filename(db_file)
        .foreign_keys(true)
        .read_only(true)
        .journal_mode(SqliteJournalMode::Delete)
        .create_if_missing(false);
    let db = SqlitePoolOptions::new().connect_lazy_with(options);
    let value = sqlx::query_scalar("select value from meta where key = \"version\"")
        .fetch(&db)
        .next()
        .await;
    if value.is_none() {
        error!("Index is incomplete, need rebuilt");
        return None;
    }
    match value.unwrap() {
        Ok::<String, _>(v) => {
            if v.as_str() != env!("CARGO_PKG_VERSION") {
                return None;
            }
            info!("Find index DB for mdict_index {}", v.as_str());
            Some(db)
        }
        Err(e) => {
            error!("Failed to query DB: {:?}", e);
            None
        }
    }
}

async fn build_db(mdx_file: impl AsRef<Path>) -> sqlx::Result<SqlitePool> {
    let db_file = mdx_file.as_ref().with_extension("db");
    info!("Build index to {}", &db_file.to_string_lossy());
    if db_file.exists() {
        fs::remove_file(&db_file)?;
    }
    let options = SqliteConnectOptions::new()
        .filename(&db_file)
        .foreign_keys(true)
        .read_only(false)
        .journal_mode(SqliteJournalMode::Wal)
        .create_if_missing(true);
    let conn = options.connect().await?;
    let index = MDictMemIndex::new(&mdx_file).map_err(sqlx::Error::Io)?;
    let builder = MDictSqliteBuilder { conn, index };
    builder.build().await?;
    // open in writeable + journal mode = delete to remove db-wal file
    let options = SqliteConnectOptions::new()
        .filename(db_file)
        .foreign_keys(true)
        .read_only(false)
        .journal_mode(SqliteJournalMode::Delete)
        .create_if_missing(false);
    let conn = options.connect().await?;
    conn.close().await?;
    let pool = open_db(mdx_file)
        .await
        .expect("Failed to open DB after build");
    Ok(pool)
}

pub struct MDictSqliteIndex {
    pool: SqlitePool,
    mdx_file: PathBuf,
    mdd_files: Vec<PathBuf>,
    header: MDictHeader,
}

impl MDictSqliteIndex {
    pub async fn new<P: AsRef<Path>>(path: P) -> sqlx::Result<MDictSqliteIndex> {
        let mdx_file = path.as_ref().canonicalize()?;
        if !mdx_file.is_file()
            || mdx_file
                .extension()
                .map(|s| s.to_str())
                .flatten()
                .map(|s| s.to_ascii_lowercase())
                != Some(String::from("mdx"))
        {
            return Err(sqlx::Error::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Expect a mdx file",
            )));
        }
        info!("mdx: {}", mdx_file.to_string_lossy());
        let pool = match open_db(&mdx_file).await {
            Some(db) => db,
            None => build_db(&mdx_file).await?,
        };
        let header = MDictHeader::new(
            OpenOptions::new().read(true).open(&mdx_file)?,
            MDictMode::Mdx,
        )?;
        let mut mdd_files = Vec::new();
        let mdd0 = mdx_file.with_extension("mdd");
        if mdd0.is_file() {
            mdd_files.push(mdd0);
            for i in 1.. {
                let ext = format!("{}.mdd", i);
                let mddi = mdx_file.with_extension(ext);
                if mddi.is_file() {
                    mdd_files.push(mddi);
                } else {
                    break;
                }
            }
        }
        for i in mdd_files.iter() {
            info!("mdd: {}", i.to_string_lossy());
        }
        Ok(MDictSqliteIndex {
            pool,
            mdx_file,
            mdd_files,
            header,
        })
    }
    pub async fn close(&self) {
        info!("Shutdown ...");
        self.pool.close().await;
    }
}

#[async_trait]
impl MDictAsyncLookup for MDictSqliteIndex {
    async fn lookup_word(&self, key: &str) -> io::Result<String> {
        let query: Option<MdxQuery> =
            sqlx::query_as("select * from mdx_index natural join mdx_block where keyword = ?1")
                .bind(key)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        match query {
            Some(result) => {
                let file: tokio::fs::File = tokio::fs::OpenOptions::new()
                    .read(true)
                    .open(&self.mdx_file)
                    .await?;
                let key = MDictRecordIndex {
                    block: result.block_index as u32,
                    offset: result.record_offset as u32,
                    len: result.record_size as u32,
                };
                let block = MDictRecordBlockIndex {
                    offset: result.block_offset as u64,
                    comp_size: result.block_size as u64,
                };
                let bytes = lookup(file, &key, &block).await?;
                let decoded = self.header.decode_string(bytes)?;
                Ok(decoded)
            }
            None => Err(io::Error::new(
                io::ErrorKind::NotFound,
                "Not found in index",
            )),
        }
    }

    async fn lookup_resource(&self, key: &str) -> io::Result<Bytes> {
        let query: Option<MddQuery> =
            sqlx::query_as("select * from mdd_index natural join mdd_block where keyword = ?1")
                .bind(key)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        match query {
            Some(result) => {
                let file: tokio::fs::File = tokio::fs::OpenOptions::new()
                    .read(true)
                    .open(&self.mdd_files[result.file_index as usize])
                    .await?;
                let key = MDictRecordIndex {
                    block: result.block_index as u32,
                    offset: result.record_offset as u32,
                    len: result.record_size as u32,
                };
                let block = MDictRecordBlockIndex {
                    offset: result.block_offset as u64,
                    comp_size: result.block_size as u64,
                };
                let data = lookup(file, &key, &block).await?;
                Ok(data)
            }
            None => Err(io::Error::new(
                io::ErrorKind::NotFound,
                "Not found in index",
            )),
        }
    }
}
