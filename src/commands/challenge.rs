use std::collections::HashMap;

use telegram_bot::*;

use crate::types::*;
use crate::{crop_letters, dropbox, update_game_message};

pub(crate) async fn process_challenge_command(
    games: &mut HashMap<String, Game>,
    api: &mut Api,
    message: &Message,
) -> Result<(), Error> {
    let game = games
        .entry(message.chat.id().to_string())
        .or_insert(Default::default());

    if let Some(reply) = &message.reply_to_message {
        if let MessageOrChannelPost::Message(ref reply) = **reply {
            let msg: GameMessage = reply.clone().into();
            if let Some((user, participant, proof)) = game.find_participant_and_proof_by_msg(&msg) {
                if let Some(challenge) = &game.proof_challenge {
                    api.send(message.text_reply(
                        format!("Голосование по трюку [уже в процессе](https://t.me/c/{chat_id}/{message_id}). \
                        Нужно дождаться его завершения.",
                            chat_id = crop_letters(&challenge.poll_msg.chat_id.to_string(), 4),
                            message_id = challenge.poll_msg.id
                        ),
                    ).parse_mode(ParseMode::MarkdownV2))
                    .await?;

                    return Ok(());
                }
                challenge_proof(game, api, &reply, &user, participant, proof).await?;
                dropbox::save_games(&games).await;
            } else {
                api.send(
                    message.text_reply("Это сообщение не представляет собою доказательство трюка."),
                )
                .await?;
            }
        }
    }

    Ok(())
}

pub(crate) async fn process_callback_query(
    games: &mut HashMap<String, Game>,
    api: Api,
    cb: CallbackQuery,
) -> Result<(), Error> {
    if let Some(message) = &cb.message {
        if let MessageOrChannelPost::Message(message) = message {
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

                    let user: GameUser = cb.from.clone().into();
                    if !game.participants.contains_key(&user) {
                        api.send(cb.answer("Голосовать могут только участники игры."))
                            .await?;
                        return Ok(());
                    }

                    if challenge.voters.contains(&user) {
                        api.send(cb.answer("Ты уже проголосовал.")).await?;
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

                    api.send(cb.answer("Твой голос принят.")).await?;

                    let voters = challenge
                        .voters
                        .iter()
                        .map(|voter| format!("[{}](tg://user?id={})", voter.first_name, voter.id))
                        .collect::<Vec<_>>()
                        .join(", ");

                    if challenge.voters.len() > game.participants.len() / 2 {
                        let result = if challenge.num_yes >= challenge.num_no {
                            (true, "✅ ПРИНЯТО")
                        } else {
                            (false, "❌ ПЕРЕДЕЛАТЬ")
                        };
                        let msg = format!(
                            "На этом видео выполнены эти трюки: {}?\n\nВердикт:*{}*\n\n_Проголосовали: {}_\n\n{} 👍, {} 👎",
                            tricks, result.1, voters, challenge.num_yes, challenge.num_no,
                        );
                        api.send(message.edit_text(msg).parse_mode(ParseMode::MarkdownV2))
                            .await?;

                        if !result.0 {
                            if let Some(participant) = game.participants.get_mut(&user) {
                                if let Some(idx) = participant
                                    .proofs
                                    .iter()
                                    .position(|proof| *proof == challenge.proof)
                                {
                                    participant.proofs.remove(idx);
                                }

                                api.send(
                                    MessageOrChannelPost::from(challenge.proof.msg.clone())
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
                        api.send(message.edit_text(msg).parse_mode(ParseMode::Markdown))
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
                dropbox::save_games(&games).await;
            }
        }
    }

    Ok(())
}

pub(crate) async fn challenge_proof(
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
