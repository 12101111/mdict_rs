use std::env;
use std::fs::{remove_file, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn main() {
    if cfg!(feature = "sqlite") {
        println!("cargo:rerun-if-changed=build.rs");
        println!("cargo:rerun-if-changed=migration/init.sql");
        println!("cargo:rerun-if-env-changed=DATABASE_URL");
        let out_dir = env::var_os("OUT_DIR").unwrap();
        let db_file = PathBuf::from(&out_dir).join("build.db");
        if db_file.exists() {
            remove_file(&db_file).unwrap();
        }
        if env::var("DATABASE_URL").is_ok() {
            return;
        }
        let mut sql = OpenOptions::new()
            .read(true)
            .open("migration/init.sql")
            .expect("SQL file not found");
        let mut sql_content = String::new();
        sql.read_to_string(&mut sql_content)
            .expect("Failed to read SQL file");
        let mut sqlite3 = Command::new("sqlite3")
            .arg(&db_file)
            .stdin(Stdio::piped())
            .spawn()
            .expect("Failed to run initial SQL");
        {
            let stdin = sqlite3.stdin.as_mut().expect("Failed to open stdin");
            stdin
                .write_all(sql_content.as_bytes())
                .expect("Failed to pipe to sqlite3");
        }
        let db_path = db_file.to_str().unwrap();
        println!("cargo:rustc-env=DATABASE_URL=sqlite://{}", db_path);
    }
}
