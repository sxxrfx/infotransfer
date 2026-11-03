mod ui;

fn main() {
    #[cfg(feature = "visual-test")]
    if std::env::args().any(|argument| argument == "--render-components") {
        ui::capture_component_previews().expect("render GPUI components");
        return;
    }
    if infotransfer::native::activate_existing() {
        return;
    }
    let _instance = match infotransfer::instance::Instance::acquire(
        &infotransfer::assets::data_dir().join("app.lock"),
    ) {
        Ok(Some(instance)) => instance,
        Ok(None) => {
            infotransfer::native::activate_existing();
            return;
        }
        Err(error) => {
            eprintln!("Could not start InfoTransfer: {error:#}");
            std::process::exit(1);
        }
    };
    ui::run();
}
