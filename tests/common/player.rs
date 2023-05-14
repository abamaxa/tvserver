use axum::http::StatusCode;
use std::sync::Arc;

use tvserver::domain::traits::{MockPlayer, MockRemotePlayer, Player, RemotePlayer};

pub fn get_player() -> Arc<dyn Player> {
    let mut player = MockPlayer::new();
    player
        .expect_send_command()
        .times(2)
        .returning(|command, _| Ok(command.to_string()));

    Arc::new(player)
}

pub fn get_remote_player() -> Arc<dyn RemotePlayer> {
    let mut player = MockRemotePlayer::new();
    player
        .expect_send()
        .times(2)
        .returning(|_| Ok(StatusCode::OK));

    Arc::new(player)
}
