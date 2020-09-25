#[macro_use]
extern crate lazy_static;

use std::collections::HashMap;
use std::env;

use futures::StreamExt;
use telegram_bot::*;
use tokio::sync::Mutex;

mod types;
use types::*;

lazy_static! {
    static ref GAMES: Mutex<HashMap<String, Game>> = Mutex::new(Default::default());
}

const MAX_TRICKS: usize = 3;
const SAVED_GAMES_FILE: &str = "games.yaml";

fn format_game_message(game: &Game) -> String {
    let participants = game
        .participants
        .iter()
        .enumerate()
        .map(|(participant_index, (participant_user, participant))| {
            let tricks = participant
                .tricks
                .iter()
                .enumerate()
                .map(|(i, trick)| {
                    format!(
                        "{}. {}{}",
                        participant_index * MAX_TRICKS + i + 1,
                        if trick.edited { "📝" } else { "" },
                        remove_markdown(&trick.name)
                    )
                })
                .collect::<Vec<String>>()
                .join("\n");

            format!(
                "🛹 {firstname} {name}\n{tricks}",
                firstname = remove_markdown(&participant_user.first_name),
                name = participant_user
                    .username
                    .clone()
                    .map(|username| format!("@{} ", username))
                    .unwrap_or("".to_owned()),
                tricks = tricks,
            )
        })
        .collect::<Vec<String>>()
        .join("\n");

    let mut leaderboard = game.participants.iter().collect::<Vec<_>>();
    leaderboard.sort_by(|(_, participant_a), (_, participant_b)| {
        let num_tricks_b: usize = participant_b.num_tricks_proven();
        let num_tricks_a: usize = participant_a.num_tricks_proven();

        num_tricks_b.cmp(&num_tricks_a)
    });

    let leaderboard = leaderboard
        .into_iter()
        .enumerate()
        .map(|(i, (user, participant))| {
            let proofs = if participant.proofs.is_empty() {
                "".to_owned()
            } else {
                let num_tricks: usize = participant.num_tricks_proven();
                let proofs = participant
                    .proofs
                    .iter()
                    .map(|proof| {
                        format!(
                            "[🎞](https://t.me/c/{chat_id}/{message_id})",
                            chat_id = crop_letters(&proof.msg.chat_id.to_string(), 4),
                            message_id = proof.msg.id,
                        )
                    })
                    .collect::<Vec<String>>()
                    .join("");

                format!(" | Пруфы: {} (трюков: {})", proofs, num_tricks)
            };

            format!(
                "{}. {firstname} {name}{proofs}",
                i + 1,
                firstname = remove_markdown(&user.first_name),
                name = user
                    .username
                    .clone()
                    .map(|username| format!("@{} ", username))
                    .unwrap_or("".to_owned()),
                proofs = proofs,
            )
        })
        .collect::<Vec<String>>()
        .join("\n");

    format!(
        "=== Game of Skate ===\n\n\
        {participants}\
        \n\n\
        === Leaderboard ===\n\n\
        {leaderboard}\
        ",
        participants = participants,
        leaderboard = leaderboard,
    )
}

async fn update_game_message(
    api: &mut Api,
    chat: &MessageChat,
    game: &mut Game,
) -> Result<(), Error> {
    let game_message_text = format_game_message(game);
    match game.game_message() {
        Some(game_message) => {
            let message: MessageOrChannelPost = game_message.into();
            let response = api
                .send(
                    message
                        .edit_text(game_message_text)
                        .parse_mode(ParseMode::Markdown),
                )
                .await?;
            game.game_message = Some(response.clone().into());

            // Ignore the error as we can't pin the message if it's pinned already
            let _ = api.send(response.pin()).await;
        }

        None => {
            let response = api
                .send(chat.text(game_message_text).parse_mode(ParseMode::Markdown))
                .await?;

            // Ignore the possible pinning error
            let _ = api.send(response.pin()).await;
            game.game_message = Some(response.into());
        }
    }

    Ok(())
}

fn extract_command(s: &str) -> Option<(String, String)> {
    let words = s.split(" ").collect::<Vec<_>>();
    if words.len() == 0 {
        return None;
    }

    let command = words[0];
    let command = command.replace("@GameOfSk8Bot", ""); // Remove bot's username from command
    let rest = words.into_iter().skip(1).collect::<Vec<_>>().join(" ");
    Some((command.to_owned(), rest))
}

async fn add_proof(
    should_accept: bool,
    message: &Message,
    rest: &str,
    sender: &GameUser,
    api: &mut Api,
    game: &mut Game,
) -> Result<(), Error> {
    if game.started() {
        if game.participant_tricks(&sender).is_some() {
            if should_accept {
                if game.proof_exists(sender, message) {
                    api.send(message.text_reply("Это видео уже добавлено."))
                        .await?;

                    return Ok(());
                }

                let tricks = rest
                    .split(",")
                    .into_iter()
                    .map(|t| t.parse::<usize>())
                    .collect::<Result<Vec<usize>, _>>();

                match tricks {
                    Ok(tricks) => {
                        let not_proven_tricks = tricks
                            .clone()
                            .into_iter()
                            .filter(|trick| !game.is_trick_proven(&sender, *trick))
                            .collect::<Vec<_>>();
                        let already_proven_tricks = tricks
                            .clone()
                            .into_iter()
                            .filter(|trick| game.is_trick_proven(&sender, *trick))
                            .collect::<Vec<_>>();

                        for trick in &already_proven_tricks {
                            if game.is_trick_proven(&sender, *trick) {
                                let trick_name =
                                    game.trick_by_number(*trick).map(|trick| trick.name);

                                api.send(message.text_reply(format!(
                                    "У тебя трюк {}уже имеет пруф! Не добавляю.",
                                    if let Some(name) = trick_name {
                                        format!("({}) ", name)
                                    } else {
                                        "".to_owned()
                                    }
                                )))
                                .await?;
                            }
                        }

                        if already_proven_tricks.len() == tricks.len() {
                            return Ok(());
                        }

                        let tricks_proven = game.prove_tricks(&sender, &message, not_proven_tricks);
                        if tricks_proven.is_empty() {
                            api.send(
                                message.text_reply(
                                    "Ни один трюк с указанным номером(-ами) не найден.",
                                ),
                            )
                            .await?;
                            return Ok(());
                        }
                        let tricks_proven = tricks_proven
                            .into_iter()
                            .map(|(number, name)| format!("{}. {}", number, name))
                            .collect::<Vec<_>>()
                            .join("\n");
                        update_game_message(api, &message.chat, game).await?;
                        api.send(
                            message.text_reply(format!("Видео-доказательство трюка добавлено в закрепленный пост. Относится к трюкам:\n{tricks}",
                                tricks = tricks_proven,
                            )),
                        )
                            .await?;
                    }
                    Err(_) => {
                        api.send(
                            message.text_reply(
                                "Один или несколько номеров трюков указаны некорректно.",
                            ),
                        )
                        .await?;
                    }
                }
            } else {
                api.send(message.text_reply(
                    "В качестве доказательства принимаются только видео либо ответ на видео.",
                ))
                .await?;
            }
        } else {
            api.send(
                message.text_reply("Сперва добавь хотя бы один свой трюк чтобы принять участие."),
            )
            .await?;
        }
    } else {
        api.send(message.text_reply(
            "Игра еще не началась! Добавь хотя бы один трюк через команду /trick <название>.",
        ))
        .await?;
    }

    Ok(())
}

fn is_video(message: &Message) -> (bool, Option<String>) {
    match message.clone().kind {
        MessageKind::Video { .. } | MessageKind::VideoNote { .. } => (true, None),

        MessageKind::Document { data, caption, .. } => data
            .mime_type
            .map(|mime| (mime == "video/mp4", caption))
            .unwrap_or((false, None)),
        _ => (false, None),
    }
}

async fn challenge_proof(
    game: &mut Game,
    api: &mut Api,
    message: &Message,
    user: &GameUser,
    participant: Participant,
    proof: Proof,
) -> Result<(), Error> {
    let tricks = proof
        .tricks_proven
        .iter()
        .flat_map(|trick_no| game.trick_by_number(*trick_no))
        .map(|trick| format!("\"{}\"", trick.name))
        .collect::<Vec<_>>()
        .join(", ");
    let mut msg = message.text_reply(format!("На этом видео выполнены эти трюки: {}?", tricks));

    let inline_keyboard = build_poll_keyboard(
        i64::from(message.chat.id()),
        user.id,
        proof.msg.id,
        None,
        None,
    );

    let msg = msg.reply_markup(inline_keyboard);

    if let MessageOrChannelPost::Message(msg) = api.send(msg).await? {
        game.proof_challenge = Some(ProofChallenge {
            user: user.clone(),
            participant,
            proof,
            poll_msg: msg.into(),
            num_yes: 0,
            num_no: 0,
            voters: Default::default(),
        });
    }

    Ok(())
}

async fn process_message(mut api: Api, message: Message) -> Result<(), Error> {
    let sender = &message.from;

    if let MessageKind::Text { ref data, .. } = message.kind {
        let res = extract_command(&data);
        if res.is_none() {
            return Ok(());
        }

        let (command, rest) = res.unwrap();
        if !command.starts_with("/") {
            // Ignore non-commands
            return Ok(());
        }

        match command.to_lowercase().as_str() {
            "/reset" => {
                let mut games = GAMES.lock().await;
                let game = games
                    .entry(message.chat.id().to_string())
                    .or_insert(Default::default());
                *game = Default::default();
                let _ = tokio::fs::write(SAVED_GAMES_FILE, serde_yaml::to_string(&*games).unwrap())
                    .await;
            }

            "/trick" | "/трюк" => {
                let mut games = GAMES.lock().await;
                let mut game = games
                    .entry(message.chat.id().to_string())
                    .or_insert(Default::default());

                match game.participant_tricks(&sender.clone().into()) {
                    Some(tricks) if tricks.len() >= MAX_TRICKS => {
                        api.send(message.text_reply(format!(
                            "У тебя все трюки уже добавлены (максимум {})",
                            MAX_TRICKS
                        )))
                        .await?;
                    }
                    _ => {
                        let trick_names = rest.replace('\n', " ").clone();
                        let trick_names = trick_names.replace('\r', " ").clone();
                        let trick_names = trick_names.trim();
                        if trick_names.trim().is_empty() {
                            api.send(message.text_reply("Название(-я) трюка не указано!"))
                                .await?;
                            return Ok(());
                        }
                        let trick_names = trick_names.split(",");
                        for trick in trick_names {
                            let num_tricks = game
                                .participant_tricks(&sender.clone().into())
                                .map(|tricks| tricks.len())
                                .unwrap_or(0);
                            if num_tricks >= MAX_TRICKS {
                                break;
                            }

                            let trick = trick.trim();
                            game.add_trick(&sender.clone().into(), trick);

                            let remaining_tricks = MAX_TRICKS - num_tricks - 1;
                            let footer = if remaining_tricks == 0 {
                                "Больше трюки добавлять нельзя.".to_owned()
                            } else {
                                format!(
                                    "Остал{} {} трюк{}.",
                                    if remaining_tricks == 1 {
                                        "ся"
                                    } else {
                                        "ось"
                                    },
                                    remaining_tricks,
                                    if remaining_tricks == 1 { "" } else { "а" },
                                )
                            };

                            api.send(
                                message
                                    .text_reply(format!("Трюк \"{}\" добавлен! {}", trick, footer)),
                            )
                            .await?;
                        }
                    }
                }

                update_game_message(&mut api, &message.chat, &mut game).await?;
                let _ = tokio::fs::write(SAVED_GAMES_FILE, serde_yaml::to_string(&*games).unwrap())
                    .await;
            }

            "/proof" | "/пруф" => {
                let mut games = GAMES.lock().await;
                let mut game = games
                    .entry(message.chat.id().to_string())
                    .or_insert(Default::default());
                let (msg, should_accept) = message
                    .clone()
                    .reply_to_message
                    .map(|reply| {
                        if let MessageOrChannelPost::Message(msg) = *reply {
                            let (is_vid, _) = is_video(&msg);
                            (Some(msg), is_vid)
                        } else {
                            (None, false)
                        }
                    })
                    .unwrap_or((None, false));
                if should_accept {
                    add_proof(
                        true,
                        &msg.unwrap(),
                        &rest,
                        &sender.clone().into(),
                        &mut api,
                        &mut game,
                    )
                    .await?;

                    let _ =
                        tokio::fs::write(SAVED_GAMES_FILE, serde_yaml::to_string(&*games).unwrap())
                            .await;
                } else {
                    add_proof(
                        false,
                        &message,
                        &rest,
                        &sender.clone().into(),
                        &mut api,
                        &mut game,
                    )
                    .await?;
                }
            }

            "/edit" => {
                let rest = rest.split(" ").collect::<Vec<_>>();
                if rest.len() < 2 {
                    api.send(message.text_reply("Нужно указать номер трюка и новое название."))
                        .await?;
                    return Ok(());
                }

                let trick_no = rest[0].parse::<usize>();
                if let Err(_) = trick_no {
                    api.send(message.text_reply("Неверно указан номер трюка."))
                        .await?;
                    return Ok(());
                }
                let trick_no = trick_no.unwrap();
                if trick_no == 0 {
                    api.send(message.text_reply("Неверно указан номер трюка."))
                        .await?;
                    return Ok(());
                }

                let trick_index = trick_no - 1;
                let mut games = GAMES.lock().await;
                let mut game = games
                    .entry(message.chat.id().to_string())
                    .or_insert(Default::default());
                let participant_index = trick_index / MAX_TRICKS;
                if let Some(user) = game.user_by_index(participant_index) {
                    if user.id != i64::from(message.from.id) {
                        api.send(message.text_reply(format!(
                            "Можно переименовывать только свои трюки ({}).",
                            (trick_index..(trick_index + MAX_TRICKS)).into_iter()
                                .map(|n| format!("№{}", n))
                                .collect::<Vec<_>>()
                                .join(", ")
                        )))
                        .await?;
                        return Ok(());
                    }

                    let new_trick_name = rest.into_iter().skip(1).collect::<Vec<_>>().join(" ");
                    match game.trick_by_number(trick_no) {
                        Some(trick) => {
                            if trick.edited {
                                api.send(
                                    message.text_reply("Трюк уже переименовывался, больше нельзя."),
                                )
                                .await?;
                                return Ok(());
                            }

                            game.update_trick_name(trick_index, new_trick_name);
                            api.send(message.text_reply("Трюк переименован!")).await?;

                            update_game_message(&mut api, &message.chat, &mut game).await?;
                            let _ = tokio::fs::write(
                                SAVED_GAMES_FILE,
                                serde_yaml::to_string(&*games).unwrap(),
                            )
                            .await;
                        }
                        None => {
                            api.send(message.text_reply("Трюк с указанным номером не найден!"))
                                .await?;
                            return Ok(());
                        }
                    }
                }
            }

            "/challenge" => {
                let mut games = GAMES.lock().await;
                let mut game = games
                    .entry(message.chat.id().to_string())
                    .or_insert(Default::default());

                if let Some(reply) = &message.reply_to_message {
                    if let MessageOrChannelPost::Message(ref reply) = **reply {
                        let msg: GameMessage = reply.clone().into();
                        if let Some((user, participant, proof)) =
                            game.find_participant_and_proof_by_msg(&msg)
                        {
                            challenge_proof(&mut game, &mut api, &reply, &user, participant, proof)
                                .await?;
                            let _ = tokio::fs::write(
                                SAVED_GAMES_FILE,
                                serde_yaml::to_string(&*games).unwrap(),
                            )
                            .await;
                        } else {
                            api.send(message.text_reply(
                                "Это сообщение не представляет собою доказательство трюка.",
                            ))
                            .await?;
                        }
                    }
                }
            }

            _ => {
                api.send(message.text_reply(
                    "Команда не опознана!\n\
                Команды:\n\
                /trick <трюк1> - добавить один трюк\n\
                /trick <трюк1, трюк2, трюк3> - добавить сразу несколько\n\
                /edit <№трюка> <новое название> - редактировать трюк (не более одного раза)\n\
                /proof - в комментарии к прикрепленному видео или в ответе на видео, \
                чтобы приобщить его в качестве доказательства\
                /challenge - в комментарии к видео-доказательству чтобы запустить голосование \
                против доказательства",
                ))
                .await?;
            }
        }
    } else {
        let (is_vid, caption) = is_video(&message);
        if !is_vid || caption.is_none() {
            return Ok(());
        }

        let caption = caption.clone().unwrap();
        let res = extract_command(&caption);
        if res.is_none() {
            return Ok(());
        }

        let (command, rest) = res.unwrap();
        if !command.starts_with("/") {
            // Ignore non-commands
            return Ok(());
        }

        match command.to_lowercase().as_str() {
            "/proof" | "/пруф" => {
                let mut games = GAMES.lock().await;
                let mut game = games
                    .entry(message.chat.id().to_string())
                    .or_insert(Default::default());
                add_proof(
                    true,
                    &message,
                    &rest,
                    &sender.clone().into(),
                    &mut api,
                    &mut game,
                )
                .await?;

                update_game_message(&mut api, &message.chat, &mut game).await?;
                let _ = tokio::fs::write(SAVED_GAMES_FILE, serde_yaml::to_string(&*games).unwrap())
                    .await;
            }
            _ => (),
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let token = env::var("TELEGRAM_BOT_TOKEN").expect("TELEGRAM_BOT_TOKEN not set");

    // Load saved games
    {
        let mut games = GAMES.lock().await;
        match std::fs::read_to_string(SAVED_GAMES_FILE) {
            Ok(json) => *games = serde_yaml::from_str(&json).unwrap_or(Default::default()),
            Err(e) => eprintln!("Failed to load saved games: {:?}", e),
        }
    }

    let api = Api::new(token);
    let mut stream = api.stream();

    // Fetch new updates via long poll method
    while let Some(update) = stream.next().await {
        let update = update?;
        match update.kind {
            UpdateKind::Message(message) => {
                let _ = process_message(api.clone(), message).await;
            }

            UpdateKind::CallbackQuery(cb) => {
                if let Some(message) = cb.message {
                    if let MessageOrChannelPost::Message(message) = message {
                        let mut games = GAMES.lock().await;
                        let game = games
                            .entry(message.chat.id().to_string())
                            .or_insert(Default::default());

                        let tricks = if let Some(challenge) = &game.proof_challenge {
                            challenge
                                .proof
                                .tricks_proven
                                .iter()
                                .flat_map(|trick_no| game.trick_by_number(*trick_no))
                                .map(|trick| format!("\"{}\"", trick.name))
                                .collect::<Vec<_>>()
                                .join(", ")
                        } else {
                            "".to_owned()
                        };

                        let mut is_resolved = false;
                        let mut should_update_game_message = false;
                        if let Some(ref mut challenge) = game.proof_challenge {
                            if let Some(data) = &cb.data {
                                let data = data.split(",").collect::<Vec<_>>();
                                let yes_no = data[0];

                                let user: GameUser = cb.from.into();
                                if challenge.voters.contains(&user)
                                    || !game.participants.contains_key(&user)
                                {
                                    return Ok(());
                                }
                                challenge.voters.insert(user.clone());

                                match yes_no {
                                    "yes" => {
                                        challenge.num_yes += 1;
                                    }
                                    "no" => {
                                        challenge.num_no += 1;
                                    }

                                    _ => return Ok(()),
                                }

                                let voters = challenge
                                    .voters
                                    .iter()
                                    .map(|voter| {
                                        if let Some(username) = &voter.username {
                                            format!("{}(@{})", voter.first_name, username)
                                        } else {
                                            voter.first_name.clone()
                                        }
                                    })
                                    .collect::<Vec<_>>()
                                    .join(", ");

                                let not_tie = challenge.num_yes != challenge.num_no;
                                if not_tie && challenge.voters.len() > game.participants.len() / 2 {
                                    let result = if challenge.num_yes > challenge.num_no {
                                        (true, "✅ ПРИНЯТО")
                                    } else {
                                        (false, "❌ ПЕРЕДЕЛАТЬ")
                                    };
                                    let msg = format!(
                                        "На этом видео выполнены эти трюки: {}?\n\nВердикт:*{}*\n\n**Проголосовали: {}**\n\n{} 👍, {} 👎",
                                        tricks, result.1, voters, challenge.num_yes, challenge.num_no,
                                    );
                                    api.send(
                                        message.edit_text(msg).parse_mode(ParseMode::Markdown),
                                    )
                                    .await?;

                                    if !result.0 {
                                        if let Some(participant) = game.participants.get_mut(&user)
                                        {
                                            if let Some(idx) = participant
                                                .proofs
                                                .iter()
                                                .position(|proof| *proof == challenge.proof)
                                            {
                                                participant.proofs.remove(idx);
                                            }

                                            api.send(
                                                MessageOrChannelPost::from(
                                                    challenge.proof.msg.clone(),
                                                )
                                                .text_reply("Это доказательство удалено."),
                                            )
                                            .await?;
                                            should_update_game_message = true;
                                        }
                                    }

                                    is_resolved = true;
                                } else {
                                    let msg = format!(
                                        "На этом видео выполнены эти трюки: {}?\n\n**Проголосовали: {}**",
                                        tricks, voters
                                    );
                                    api.send(
                                        message.edit_text(msg).parse_mode(ParseMode::Markdown),
                                    )
                                    .await?;

                                    let keyboard = build_poll_keyboard(
                                        challenge.poll_msg.chat_id,
                                        challenge.user.id,
                                        challenge.proof.msg.id,
                                        Some(challenge.num_yes),
                                        Some(challenge.num_no),
                                    );

                                    api.send(message.edit_reply_markup(Some(keyboard))).await?;
                                }
                            }
                        }

                        if is_resolved {
                            game.proof_challenge = None;

                            if should_update_game_message {
                                update_game_message(&mut api.clone(), &message.chat, game).await?;
                            }
                            let _ = tokio::fs::write(
                                SAVED_GAMES_FILE,
                                serde_yaml::to_string(&*games).unwrap(),
                            )
                            .await;
                        }
                    }
                }
            }

            _ => (),
        }
    }

    Ok(())
}

fn build_poll_keyboard(
    chat_id: i64,
    user_id: i64,
    proof_msg_id: i64,
    num_yes: Option<usize>,
    num_no: Option<usize>,
) -> InlineKeyboardMarkup {
    let yes_button_caption = format!(
        "👍 Да{}",
        if let Some(num_yes) = num_yes {
            format!(" ({})", num_yes)
        } else {
            "".to_owned()
        }
    );
    let yes_button_data = format!("yes,{},{},{}", chat_id, user_id, proof_msg_id);

    let no_button_caption = format!(
        "👎 Нет{}",
        if let Some(num_no) = num_no {
            format!(" ({})", num_no)
        } else {
            "".to_owned()
        }
    );
    let no_button_data = format!("no,{},{},{}", chat_id, user_id, proof_msg_id);

    reply_markup!(inline_keyboard,
        [yes_button_caption callback yes_button_data, no_button_caption callback no_button_data]
    )
}

fn crop_letters(s: &str, pos: usize) -> &str {
    match s.char_indices().skip(pos).next() {
        Some((pos, _)) => &s[pos..],
        None => "",
    }
}

fn remove_markdown(s: &str) -> String {
    let s = s.replace('*', "");
    let s = s.replace('[', "");
    let s = s.replace(']', "");
    let s = s.replace('~', "");
    let s = s.replace('`', "");
    let s = s.replace('(', "");
    let s = s.replace(')', "");
    s
}
