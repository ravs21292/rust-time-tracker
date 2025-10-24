use prost::Message;

pub mod auth {
    include!(concat!(env!("OUT_DIR"), "/auth.rs"));
}

pub mod activity {
    include!(concat!(env!("OUT_DIR"), "/activity.rs"));
}

pub mod screenshot {
    include!(concat!(env!("OUT_DIR"), "/screenshot.rs"));
}
