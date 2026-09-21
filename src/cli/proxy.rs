//! CLI proxy scope reporting.

use a3s_search::providers::BuiltinProvider;

pub(crate) fn report_proxy_scope(
    proxy: Option<&str>,
    engine_shortcuts: &[String],
    text_output: bool,
) {
    if proxy.is_none() {
        return;
    }

    let uses_provider = engine_shortcuts
        .iter()
        .any(|shortcut| BuiltinProvider::from_id(shortcut).is_some());
    let uses_conventional = engine_shortcuts
        .iter()
        .any(|shortcut| BuiltinProvider::from_id(shortcut).is_none());

    if uses_provider && uses_conventional {
        eprintln!(
            "Proxy enabled for conventional engines; native provider API requests remain direct"
        );
    } else if uses_provider {
        eprintln!("The --proxy option does not apply; native provider API requests remain direct");
    } else if text_output {
        eprintln!("Proxy enabled for conventional engines");
    }
}
