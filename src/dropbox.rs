use std::collections::HashMap;

use dropbox_sdk::files::{download, upload, CommitInfo, DownloadArg, WriteMode};
use dropbox_sdk::HyperClient;

use crate::types::Game;

const FILE_NAME: &str = "/Apps/skate-tg-bot/games.yaml";

pub(crate) async fn load_games() -> HashMap<String, Game> {
    let token = std::env::var("DROPBOX_OAUTH_TOKEN").expect("Dropbox OAuth token");
    let client = HyperClient::new(token);

    return tokio::spawn(async move {
        let arg = DownloadArg {
            path: FILE_NAME.to_owned(),
            rev: None,
        };

        let download_result = download(&client, &arg, None, None).unwrap();
        return match download_result {
            Ok(res) => {
                let body = res.body;
                if let Some(body) = body {
                    serde_yaml::from_reader(body).unwrap()
                } else {
                    Default::default()
                }
            }

            Err(e) => {
                eprintln!("Dropbox download error: {:?}", e);
                Default::default()
            }
        };
    })
    .await
    .unwrap();
}

pub(crate) async fn save_games(games: &HashMap<String, Game>) {
    let token = std::env::var("DROPBOX_OAUTH_TOKEN").expect("Dropbox OAuth token");
    let client = HyperClient::new(token);

    let cloned_games = games.clone();
    return tokio::spawn(async move {
        let arg = CommitInfo {
            path: FILE_NAME.to_owned(),
            mode: WriteMode::Overwrite,
            autorename: false,
            client_modified: None,
            mute: false,
            property_groups: None,
            strict_conflict: false,
        };

        let body = serde_yaml::to_string(&cloned_games).unwrap();
        upload(&client, &arg, body.as_bytes()).unwrap().unwrap();
    })
    .await
    .unwrap();
}
