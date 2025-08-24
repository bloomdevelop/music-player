use cosmic::prelude::*;
use cosmic::widget;
use cosmic::iced::Length;
use cosmic::iced::alignment::{Horizontal};

use super::super::{AppModel, Message};

pub fn now_playing_view(app: &AppModel) -> Element<'_, Message> {
    // Read metadata for current track if available
    let (title, artist, album) = if let Some(player) = &app.audio {
        let md = player.metadata();
        (
            md.title.unwrap_or_else(|| "Unknown Title".into()),
            md.artist.unwrap_or_else(|| "Unknown Artist".into()),
            md.album.unwrap_or_else(|| "Unknown Album".into()),
        )
    } else {
        (
            String::from("Unknown Title"),
            String::from("Unknown Artist"),
            String::from("Unknown Album"),
        )
    };
    let play = widget::button::standard("Play").on_press(Message::Play);
    let pause = widget::button::suggested("Pause").on_press(Message::Pause);
    let stop = widget::button::destructive("Stop").on_press(Message::Stop);
    let prev = widget::button::standard("Prev").on_press(Message::Prev);
    let next = widget::button::standard("Next").on_press(Message::Next);

    widget::column()
        .spacing(12)
        .push(widget::text::title1("Now Playing"))
        .push(widget::text(format!("{}", title)))
        .push(widget::text(format!("{} — {}", artist, album)))
        .push(widget::row().spacing(8).push(prev).push(play).push(pause).push(stop).push(next))
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Horizontal::Center)
        .into()
}
