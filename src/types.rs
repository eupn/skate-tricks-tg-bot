use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use telegram_bot::*;

use crate::MAX_TRICKS;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct GameMessage {
    pub id: i64,
    pub chat_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Proof {
    pub msg: GameMessage,
    pub tricks_proven: Vec<usize>, // Contains trick numbers
}

impl Proof {
    pub fn new(msg: &GameMessage, tricks_proven: Vec<usize>) -> Self {
        Proof {
            msg: msg.clone(),
            tricks_proven,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Trick {
    pub name: String,
    pub edited: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Participant {
    pub tricks: Vec<Trick>,
    pub proofs: Vec<Proof>,
}

impl Participant {
    pub fn num_tricks_proven(&self) -> usize {
        self.proofs
            .clone()
            .into_iter()
            .map(|proof| proof.tricks_proven.len())
            .sum()
    }
}

/// This object represents a Telegram user or bot.
#[derive(Debug, Clone, PartialEq, PartialOrd, Eq, Ord, Hash, Serialize, Deserialize)]
pub(crate) struct GameUser {
    /// User's ID.
    pub id: i64,
    /// User‘s or bot’s first name.
    pub first_name: String,
    /// User‘s or bot’s username.
    pub username: Option<String>,
}

impl From<User> for GameUser {
    fn from(u: User) -> Self {
        GameUser {
            id: u.id.into(),
            first_name: u.first_name,
            username: u.username,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Game {
    pub participants: IndexMap<GameUser, Participant>,
    pub game_message: Option<GameMessage>,
    pub is_started: bool,
}

impl Default for Game {
    fn default() -> Self {
        Game {
            participants: Default::default(),
            game_message: None,
            is_started: false,
        }
    }
}

impl Game {
    pub fn started(&self) -> bool {
        self.is_started
    }

    pub fn participant_tricks(&self, participant: &GameUser) -> Option<Vec<Trick>> {
        self.participants
            .get(participant)
            .map(|participant| participant.tricks.clone())
    }

    pub fn add_trick(&mut self, participant: &User, trick: &str) {
        let participant = self
            .participants
            .entry(participant.clone().into())
            .or_insert(Participant {
                tricks: vec![],
                proofs: vec![],
            });

        (*participant).tricks.push(Trick {
            name: trick.to_owned(),
            edited: false,
        });

        // Start game if it's not started yet
        if !self.is_started {
            self.is_started = true;
        }
    }

    pub fn trick_by_number(&self, number: usize) -> Option<Trick> {
        if number == 0 {
            return None;
        }

        let number = number - 1; // From human numbers to indices
        let participant_index = number / MAX_TRICKS;
        let trick_index = number % MAX_TRICKS;
        let participant = self.participants.values().nth(participant_index);
        participant.and_then(|participant| participant.tricks.get(trick_index).cloned())
    }

    pub fn update_trick_name(&mut self, index: usize, new_name: String) {
        let participant_index = index / MAX_TRICKS;
        let trick_index = index % MAX_TRICKS;
        let mut participant = self.participants.values_mut().nth(participant_index);
        if let Some(ref mut participant) = &mut participant {
            if let Some(trick) = participant.tricks.get_mut(trick_index) {
                std::mem::replace(
                    trick,
                    Trick {
                        name: new_name,
                        edited: true,
                    },
                );
            }
        }
    }

    pub fn prove_tricks(
        &mut self,
        participant: &GameUser,
        message: &Message,
        tricks: Vec<usize>,
    ) -> Vec<(usize, String)> {
        let trick_names: Vec<_> = tricks
            .iter()
            .map(|number| (*number, self.trick_by_number(*number)))
            .filter(|(_, trick)| trick.is_some())
            .map(|(number, trick)| (number, trick.unwrap().name))
            .collect();

        if trick_names.is_empty() {
            return trick_names;
        }

        self.participants.get_mut(participant).map(|participant| {
            (*participant)
                .proofs
                .push(Proof::new(&message.clone().into(), tricks));
        });

        trick_names
    }

    pub fn is_trick_proven(&self, participant: &GameUser, trick: usize) -> bool {
        self.participants
            .get(participant)
            .and_then(|participant| {
                participant.proofs.iter().find(|proof| {
                    proof
                        .tricks_proven
                        .iter()
                        .find(|number| **number == trick)
                        .is_some()
                })
            })
            .is_some()
    }

    pub fn proof_exists(&mut self, participant: &GameUser, src_message: &Message) -> bool {
        self.participants
            .get(participant)
            .and_then(|participant| {
                participant.proofs.iter().find(|proof| {
                    let src_message_id: i64 = src_message.id.into();
                    let src_chat_id: i64 = src_message.chat.id().into();
                    proof.msg.id == src_message_id && proof.msg.chat_id == src_chat_id
                })
            })
            .is_some()
    }

    pub fn game_message(&self) -> Option<GameMessage> {
        self.game_message.clone()
    }

    pub fn user_by_index(&self, index: usize) -> Option<GameUser> {
        self.participants.keys().nth(index).cloned()
    }
}

impl From<GameMessage> for MessageOrChannelPost {
    fn from(m: GameMessage) -> Self {
        MessageOrChannelPost::Message(Message {
            id: MessageId::new(m.id),
            from: User {
                id: UserId::new(0),
                first_name: "".to_string(),
                last_name: None,
                username: None,
                is_bot: false,
                language_code: None,
            },
            date: 0,
            chat: MessageChat::Supergroup(Supergroup {
                id: SupergroupId::new(m.chat_id),
                title: "".to_string(),
                username: None,
                invite_link: None,
            }),
            forward: None,
            reply_to_message: None,
            edit_date: None,
            kind: MessageKind::DeleteChatPhoto,
        })
    }
}

impl From<Message> for GameMessage {
    fn from(m: Message) -> Self {
        GameMessage {
            id: m.id.into(),
            chat_id: m.chat.id().into(),
        }
    }
}

impl From<MessageOrChannelPost> for GameMessage {
    fn from(mocp: MessageOrChannelPost) -> Self {
        match mocp {
            MessageOrChannelPost::Message(msg) => msg.into(),
            MessageOrChannelPost::ChannelPost(_) => unimplemented!(),
        }
    }
}
