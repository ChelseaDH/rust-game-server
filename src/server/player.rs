use crate::connection::Connection;

pub const PLAYER_ONE_ID: u8 = 1;
pub const PLAYER_TWO_ID: u8 = 2;

#[derive(Debug)]
pub struct Player {
    id: u8,
    pub(crate) connection: Connection,
}

impl Player {
    pub fn new_player_one(connection: Connection) -> Player {
        Player {
            id: PLAYER_ONE_ID,
            connection,
        }
    }

    pub fn new_player_two(connection: Connection) -> Player {
        Player {
            id: PLAYER_TWO_ID,
            connection,
        }
    }
}

impl PartialEq for Player {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

pub fn get_alternative_player_id(player_id: u8) -> u8 {
    if player_id == PLAYER_ONE_ID {
        PLAYER_TWO_ID
    } else {
        PLAYER_ONE_ID
    }
}
