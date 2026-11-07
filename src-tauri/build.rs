// Generate a small preview icon before Tauri's compile-time context reads it.
// A proper platform icon set will be committed for production packaging.
const ICON_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAEAAAABACAYAAACqaXHeAAABhUlEQVR42u2bMQ7CMAxFW6tiQuJwSFyCjYNwEM7FBZCYGICpC2qondpO4vxsiMr9/yVN6zgZz8f7Z+i40dB5AwAA6LxNGkF2tC8i/vV+lgNQynRKQy6MqUXj/3RJQVAE81s0UiTzOVopmnmpZopoXqKdoprnesCXYOTe53iZSgi63g7J/y6nB3KB4gAiDf81TxgBAAAAAAAAAAAAAAAAAAAAAAAAXjl/DfFMAcxitURrxzMF8Ctyq2jteKYAUuJyRWvHMwWwJkoqWjueOQDOYiZXNOc6rcXTcWmT1JY1QavZWsP8UuWYahFXKj61INIyLrXSU1ZQqYXhavlYmecCW8VbzykuyVCuCY86oVs2KDXjVSR1TYe5pjwrxO7rAWvmqiiPa+zAlJicf1uaT3kaU+cFopXIUwDUd4jUnAuI5gDrx6CG3i8yCdbWKJdchN5njYCWIXC0j9xTY629FbgdR9oBWzIvngRbgCDVOOXeoLZHwu3IzNINuzw0FeVNgQ8hAACAvtsX7FqaUIOoeA8AAAAASUVORK5CYII=";

fn main() {
    use base64::Engine;
    let folder = std::path::Path::new("icons");
    std::fs::create_dir_all(folder).expect("create icon folder");
    let icon = folder.join("icon.png");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(ICON_BASE64)
        .expect("decode embedded icon");
    std::fs::write(icon, bytes).expect("write preview icon");
    tauri_build::build();
}
