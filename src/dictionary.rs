use itertools::Itertools;
use std::collections::HashSet;
use tokio::{
    fs::File,
    io::{AsyncBufReadExt, BufReader},
    sync::OnceCell,
};

static WORDS: OnceCell<HashSet<String>> = OnceCell::const_new();

static WORD_LIST_RAW: &str = include_str!("../words");

pub async fn dictionary() -> &'static HashSet<String> {
    WORDS
        .get_or_init(|| async {
            let mut set = HashSet::new();
            match std::env::var("WORD_LIST_URL") {
                Ok(url) => {
                    let body = reqwest::get(url).await.unwrap().text().await.unwrap();
                    for line in body.lines() {
                        set.insert(line.to_uppercase());
                    }
                }
                Err(_) => {
                    let lines = WORD_LIST_RAW.lines();

                    for line in lines {
                        set.insert(line.to_uppercase());
                    }
                }
            }
            set
        })
        .await
}

pub async fn illegal_words(words: Vec<String>) -> Vec<String> {
    let dict = dictionary().await;

    words
        .into_iter()
        .filter(|word| !dict.contains(&*word))
        .collect()
}

// formatted for display
pub(crate) async fn two_letter_words() -> String {
    let dict = dictionary().await;
    let mut res = dict
        .iter()
        .filter(|word| word.len() == 2)
        .cloned()
        .collect::<Vec<_>>();

    res.sort();
    let mut chunks = res.into_iter().chunk_by(|a| a.chars().next().unwrap());
    chunks
        .into_iter()
        .map(|(_, mut vals)| vals.join(" "))
        .collect::<Vec<_>>()
        .join("\n")
}
