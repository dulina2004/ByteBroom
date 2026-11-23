fn main() {
    let config = slint_build::CompilerConfiguration::new()
        .with_style("fluent-dark".into())
        .with_edition((1, 19));
    slint_build::compile_with_config("ui/app-window.slint", config)
        .expect("failed to compile the ByteBroom interface");
}
