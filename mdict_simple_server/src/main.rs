use mdict_index::{MDictAsyncLookup, MDictSqliteIndex};
use regex::Regex;
use std::{env, path::Path, sync::Arc};
use warp::{filters::path::Tail, http::Response, Filter};

#[tokio::main]
async fn main() {
    let file = env::args().nth(1).unwrap().to_owned();
    if env::var_os("RUST_LOG").is_none() {
        env::set_var(
            "RUST_LOG",
            "warn,mdict=info,mdict_index=info,main=info,warp=info",
        );
    }
    pretty_env_logger::init();
    let log = warp::log("main");
    let dir = Path::new(&file)
        .canonicalize()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned();
    let mdict = Arc::new(MDictSqliteIndex::new(&file).await.unwrap());
    let mdict_clone = mdict.clone();
    let shared = warp::any().map(move || (mdict.clone()));
    let mdict_server = warp::path::tail().and(shared).and_then(
        |path: Tail, mdict: Arc<MDictSqliteIndex>| async move {
            let path = path.as_str();
            let mime = mime_guess::from_path(path).first();
            let lookup = match mime.as_ref() {
                Some(_) => mdict.lookup_resource(path).await,
                None => {
                    let key = urlencoding::decode(path).unwrap();
                    let result = mdict.lookup_word(&key).await;
                    result.map(|opt| {
                        let string = Regex::new("(entry|sound):/").unwrap().replace_all(&opt, "");
                        let string = Regex::new("@@@LINK=([\\w]+)").unwrap().replace_all(
                            &string,
                            |link: &regex::Captures| {
                                format!("<a href=\"/{}\" >See also: {}</a>", &link[1], &link[1])
                            },
                        );
                        string.to_string().into()
                    })
                }
            };
            let content_type = mime.unwrap_or(mime::TEXT_HTML_UTF_8);
            match lookup {
                Ok(data) => Ok(Response::builder()
                    .header("content-type", content_type.to_string())
                    .body(data)
                    .unwrap()),
                Err(e) => {
                    if e.kind() != std::io::ErrorKind::NotFound {
                        log::error!("lookup {} failed : {}", path, e);
                    }
                    Err(warp::reject::not_found())
                }
            }
        },
    );
    let routes = warp::get()
        .and(mdict_server)
        .or(warp::fs::dir(dir))
        .with(log);
    let (tx, rx) = tokio::sync::oneshot::channel();
    let (addr, server) =
        warp::serve(routes).bind_with_graceful_shutdown(([127, 0, 0, 1], 8080), async move {
            rx.await.ok();
            mdict_clone.close().await;
        });
    tokio::task::spawn(server);
    log::info!("MDict Server start at {}", addr);
    tokio::signal::ctrl_c()
        .await
        .expect("failed to capture ctrl-c signal");
    log::info!("Exiting ...");
    let _ = tx.send(());
}
