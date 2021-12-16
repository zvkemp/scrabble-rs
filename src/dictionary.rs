use std::collections::HashSet;
use tokio::{
    fs::File,
    io::{AsyncBufReadExt, BufReader},
    sync::OnceCell,
};

static WORDS: OnceCell<HashSet<String>> = OnceCell::const_new();

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
                    let file = File::open("./words").await.unwrap();
                    let reader = BufReader::new(file);
                    let mut lines = reader.lines();

                    while let Ok(Some(line)) = lines.next_line().await {
                        set.insert(line.to_uppercase());
                    }
                }
            }
            set
        })
        .await
}

pub async fn illegal_words<'a>(words: Vec<String>) -> Vec<String> {
    let dict = dictionary().await;

    words
        .into_iter()
        .filter(|word| !dict.contains(&*word))
        .collect()
}
