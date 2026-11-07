// Generate a small preview icon before Tauri's compile-time context reads it.
// A proper platform icon set will be committed for production packaging.
const ICON_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAEAAAABACAYAAACqaXHeAAABEUlEQVR42u3b4Q2CMBCGYdqwAYvhGDqQa7iYzqC/TAxJocB9J+Xe/jEGc949louUkq7j890FHrkLPgAAAAAAAAAAAAAAACDo6P/xpffHUDx2u7yYAQAAAAAAAAAAAAAAAAAAAAAAAECbAHOLHkeIJwX4JmuVtHU8KcA0yb1JW8eTApSS25q0dTwpwFJSa5O2jicHqFnNrU265nNWq8fJeo+Qqlsripc0QfW6vnX83EKSyri5lV9KhZpbmK7K04prgaN3bHVTdZkBW4vwuFPsdgqsLcbrNrlrD6gtynOPgHsTnBa39P50AL9Fll49R4r+vIDJHiGPCyBVv+CPEAAAxB6Jp8Y4BQAAAAAAAAAAgKDjA4F1WFnsZfS0AAAAAElFTkSuQmCC";

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
