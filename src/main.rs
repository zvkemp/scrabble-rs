use axum::{
    extract::{Extension, WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
    AddExtensionLayer, Router,
};
use axum_channels::{
    channel::{self, ChannelBehavior},
    message::{DecoratedMessage, Message, MessageReply},
    registry::Registry,
    types::Token,
    ConnFormat,
};
use scrabble::{Game, Player};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::UnboundedSender;

mod scrabble;

#[tokio::main]
async fn main() {
    let registry = Arc::new(Mutex::new(Registry::default()));
    let lobby = Box::new(Lobby(registry.clone()));

    let mut locked = registry.lock().unwrap();

    locked.add_channel("lobby".to_string(), lobby);

    drop(locked);

    let app = Router::new()
        .route("/ws", get(json_handler))
        .route("/simple", get(simple_handler))
        .layer(AddExtensionLayer::new(registry));

    axum::Server::bind(&"0.0.0.0:3000".parse().unwrap())
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn json_handler(
    ws: WebSocketUpgrade,
    Extension(registry): Extension<Arc<Mutex<Registry>>>,
) -> impl IntoResponse {
    println!("handler");
    ws.on_upgrade(move |socket| {
        axum_channels::handle_connect(socket, ConnFormat::JSON, registry.clone())
    })
}

async fn simple_handler(
    ws: WebSocketUpgrade,
    Extension(registry): Extension<Arc<Mutex<Registry>>>,
) -> impl IntoResponse {
    println!("simple_handler");
    ws.on_upgrade(move |socket| {
        axum_channels::handle_connect(socket, ConnFormat::Simple, registry.clone())
    })
}

#[derive(Debug)]
struct Lobby(Arc<Mutex<Registry>>);

impl ChannelBehavior for Lobby {
    fn handle_message(&mut self, message: &DecoratedMessage) -> Option<Message> {
        match &message.inner {
            Message::Channel { text, .. } => {
                return self.handle_message_inner(
                    message.token,
                    message.reply_to.as_ref(),
                    text.to_string(),
                );
            }
            // these should never happen
            Message::Join { .. } => todo!(),
            Message::Leave { .. } => todo!(),
            Message::DidJoin { .. } => todo!(),
            Message::Reply(_) => todo!(),
            Message::Broadcast(_) => todo!(),
        };

        None
    }
}

impl Lobby {
    fn handle_message_inner(
        &mut self,
        token: Token,
        reply_to: Option<&UnboundedSender<Message>>,
        text: String,
    ) -> Option<Message> {
        let mut tokens = text.split_whitespace();
        match tokens.next() {
            Some("new_game") => {
                let name = tokens.next().unwrap(); // FIXME

                let mut locked = self.0.lock().unwrap();
                let game = Box::new(GameChannel::new());

                // FIXME: something something security
                locked.add_channel(name.to_string(), game);

                reply_to.map(|sender| {
                    sender.send(Message::Join {
                        channel_id: name.to_string(),
                    });
                });

                return Some(Message::Broadcast(format!("{:#?}", self)));
                // FIXME: ensure socket joins game!
            }
            _ => {
                eprintln!("unknown command {:?}", text);
                return None;
            }
        }
    }
}

struct GameChannel {
    pub(crate) game: Game,
}

impl GameChannel {
    pub fn new() -> Self {
        GameChannel {
            game: Game::default(),
        }
    }
}

impl ChannelBehavior for GameChannel {
    fn handle_message(&mut self, _message: &DecoratedMessage) -> Option<Message> {
        None
    }

    fn handle_join(&mut self, message: &DecoratedMessage) -> Result<(), channel::JoinError> {
        let player = Player(format!("{:?}", message.token));
        self.game
            .add_player(player)
            .map_err(|_| channel::JoinError::Unknown)?;

        println!("{:#?}", self.game);

        Ok(())
    }
}
