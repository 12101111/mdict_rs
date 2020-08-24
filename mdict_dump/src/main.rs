//! # mdict_dump
//!
//! This simple program load mdx and mdd files and then print all keywords in 
//! mdx file to stdout.
//!
//! # Usage
//!
//! ```shell
//! mdict_dump [PATH TO MDX FILE]
//! ```
//!
//! # panic
//! 
//! This program will panic if the mdx file is invalid or can't be opened by `mdict`

use mdict_index::*;
use std::{env, path::Path};
fn main() {
    let file = env::args().nth(1).unwrap();
    if env::var_os("RUST_LOG").is_none() {
        env::set_var("RUST_LOG", "info");
    }
    pretty_env_logger::init();
    let mdx_file = Path::new(&file).canonicalize().unwrap();
    let index = MDictMemIndex::new(mdx_file).unwrap();
    for i in index.keyword_iter() {
        println!("{}", i);
    }
}
