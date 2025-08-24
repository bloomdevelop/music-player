use cosmic::prelude::*;
use cosmic::widget;
use cosmic::widget::icon;
use cosmic::iced::Length;
use cosmic::iced::alignment::{Horizontal, Vertical};

use super::super::{AppModel, Message};

pub fn library_view(app: &AppModel) -> Element<'_, Message> {
    // Rows
    let mut rows = widget::column().spacing(4);
    for path in app.library_tracks().iter().take(200) {
        let label = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());

        let play_btn = widget::button::icon(icon::from_name("media-playback-start-symbolic"))
            .on_press(Message::LoadPath(path.to_string_lossy().into_owned()));

        let add_btn = widget::button::icon(icon::from_name("list-add-symbolic"))
            .on_press(Message::Enqueue(path.to_string_lossy().into_owned()));

        let row = widget::row()
            .spacing(8)
            .align_y(Vertical::Center)
            .push(play_btn)
            .push(add_btn)
            .push(widget::text(label).width(Length::Fill))
            .width(Length::Fill);

        rows = rows.push(widget::container(row).padding([4, 8]));
    }

    let library = widget::column()
        .push(widget::scrollable(rows).height(Length::FillPortion(1)));

    widget::column()
        .spacing(12)
        .push(library)
        .apply(widget::container)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Horizontal::Left)
        .align_y(Vertical::Top)
        .into()
}
