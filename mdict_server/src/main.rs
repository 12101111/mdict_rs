use bytes::Bytes;
use mdict_index::{MDictAsyncLookup, MDictSqliteIndex};
use regex::Regex;
use std::{
    env,
    fmt::Write,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::prelude::*;
use warp::{filters::path::Tail, http::Response, Filter};

const MDICT_JS: &str = include_str!("../static/mdict.js");

#[tokio::main]
async fn main() {
    let config_path = env::args().nth(1).unwrap().to_owned();
    let mut config_file = File::open(&config_path).unwrap();
    let mut config = String::new();
    config_file.read_to_string(&mut config).unwrap();
    if env::var_os("RUST_LOG").is_none() {
        env::set_var(
            "RUST_LOG",
            "warn,mdict=info,mdict_index=info,main=info,warp=info",
        );
    }
    pretty_env_logger::init();
    let log = warp::log("main");
    let mut indexes = Vec::new();
    let mut paths = Vec::new();
    for path in config.lines() {
        let mdict = MDictSqliteIndex::new(&path).await.unwrap();
        indexes.push(mdict);
        let dir = Path::new(&path)
            .canonicalize()
            .unwrap()
            .parent()
            .unwrap()
            .to_owned();
        paths.push(dir);
    }
    let indexes = Arc::new(indexes);
    let paths = Arc::new(paths);
    let indexes_clone = indexes.clone();
    let indexes_shared = warp::any().map(move || indexes_clone.clone());
    let indexes_shared2 = warp::any().map(move || indexes.clone());
    let paths_shared = warp::any().map(move || paths.clone());
    let mdict_server = warp::path::param()
        .and(warp::path::tail())
        .and(indexes_shared)
        .and_then(
            |i: usize, path: Tail, mdict: Arc<Vec<MDictSqliteIndex>>| async move {
                if i >= mdict.len() {
                    return Err(warp::reject::not_found());
                }
                let path = path.as_str();
                log::info!("load: {:?}/{:?}", i, path);
                let mime = mime_guess::from_path(path)
                    .first()
                    .unwrap_or(mime::TEXT_HTML_UTF_8);
                match mdict[i].lookup_resource(path).await {
                    Ok(mut data) => {
                        if mime == mime::TEXT_CSS || mime == mime::TEXT_CSS_UTF_8 {
                            data = fix_css(i, data);
                        }
                        Ok(Response::builder()
                            .header("content-type", mime.to_string())
                            .body(data)
                            .unwrap())
                    }
                    Err(e) => {
                        if e.kind() != std::io::ErrorKind::NotFound {
                            log::error!("load {} failed : {}", path, e);
                        }
                        Err(warp::reject::not_found())
                    }
                }
            },
        );
    let files = warp::path!(usize / String)
        .and(warp::path::end())
        .and(paths_shared)
        .and_then(
            |i: usize, uri: String, paths: Arc<Vec<PathBuf>>| async move {
                if i >= paths.len() {
                    return Err(warp::reject::not_found());
                }
                log::info!("load files: {:?}/{:?}", i, uri);
                let mut file = paths[i].clone();
                file.push(&uri);
                if file.exists() {
                    let mut file = tokio::fs::File::open(&file)
                        .await
                        .map_err(|_| warp::reject::not_found())?;
                    let mut data = Vec::new();
                    file.read_to_end(&mut data)
                        .await
                        .map_err(|_| warp::reject::not_found())?;
                    let mime = mime_guess::from_path(uri).first();
                    let mime = mime.unwrap_or(mime::TEXT_HTML_UTF_8);
                    let data = if mime == mime::TEXT_CSS || mime == mime::TEXT_CSS_UTF_8 {
                        fix_css(i, data.into())
                    } else {
                        data.into()
                    };
                    Ok(Response::builder()
                        .header("content-type", mime.to_string())
                        .body(data)
                        .unwrap())
                } else {
                    Err(warp::reject::not_found())
                }
            },
        );
    let lookup = warp::path::param().and( warp::path::end()).and(indexes_shared2).and_then(
        |keyword: String, mdict: Arc<Vec<MDictSqliteIndex>>| async move {
            let key = urlencoding::decode(&keyword).unwrap();
            log::info!("lookup: {:?}", key);
            let mut body = format!(r#"<!DOCTYPE html><html><head><meta charset="utf-8"><title>{}</title></head><body>"#, keyword);
            let mut no_result = true;
            for (i,dict) in mdict.iter().enumerate() {
                let result = dict.lookup_word(&key).await;
                let content = match result {
                    Ok(result) => result,
                    Err(e) => {
                        if e.kind() != std::io::ErrorKind::NotFound {
                            log::error!("lookup {} failed : {}", key, e);
                        }
                        continue;
                    },
                };
                no_result = false;
                let content = Regex::new(r#"(src|href)\s*=\s*"(file://|sound:/|entry:/)?/?([^"]+)""#)
                    .unwrap()
                    .replace_all(&content, |link: &regex::Captures| {
                        if link[3].contains("data:") {
                            return link[0].to_string()
                        }
                        match link.get(2){
                            Some(m) => {
                                let proto = m.as_str();
                                match proto {
                                    "sound:/" => format!(r#"{}="sound://{}/{}""#,&link[1], i, &link[3]),
                                    "entry:/" => format!(r#"{}="/{}""#,&link[1], &link[3]),
                                    _ =>format!(r#"{}="/{}/{}""#,&link[1], i, &link[3])
                                }
                            }
                            None => format!(r#"{}="/{}/{}""#,&link[1], i, &link[3])
                        }
                    });
                let content = Regex::new("@@@LINK=([\\w\\s]+)").unwrap().replace_all(
                    &content,
                    |link: &regex::Captures| {
                        format!(
                            "<a href=\"/{}\" >See also: {}</a>",
                            &link[1], &link[1]
                        )
                    },
                );
                write!(body, r#"<div id="mdict_rs_{}">{}</div>"#,i,content).unwrap();
            }
            if no_result {
                return Err(warp::reject::not_found())
            }
            body.push_str(r#"</body><script>"#);
            body.push_str(MDICT_JS);
            body.push_str(r#"</script></html>"#);
            Ok(warp::reply::html(body))
        },
    );
    let routes = warp::get().and(mdict_server).or(files).or(lookup).with(log);
    warp::serve(routes).run(([0, 0, 0, 0], 8080)).await;
}

// from flask-mdict
fn fix_css(id: usize, css: Bytes) -> Bytes {
    let css = std::str::from_utf8(&css).unwrap();
    // remove comments, https://stackoverflow.com/questions/9329552/explain-regex-that-finds-css-comments
    let css = Regex::new(r#"(/\*[^*]*\*+([^/*][^*]*\*+)*/)"#)
        .unwrap()
        .replace_all(&css, "");
    let css =
        Regex::new(r#"\s*([^}/;]+?)\s*\{"#)
            .unwrap()
            .replace_all(&css, |caps: &regex::Captures| {
                let tags = &caps[1];
                if tags.starts_with("@") {
                    caps[0].to_string()
                } else {
                    let mut result = "\n".to_string();
                    for tag in tags.split(',') {
                        let tag = tag.trim();
                        write!(&mut result, "#mdict_rs_{} {},", id, tag).unwrap();
                    }
                    result.pop();
                    result.push('{');
                    result
                }
            });
    css.to_string().into()
}

// TODO: cache page using lru
// TODO: build regex using once_cell
