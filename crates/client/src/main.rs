use macroquad::prelude::*;

#[macroquad::main("Ferraria")]
async fn main() {
    loop {
        clear_background(Color::from_rgba(110, 170, 230, 255));
        draw_text("Ferraria", 40.0, 60.0, 48.0, WHITE);
        next_frame().await;
    }
}
