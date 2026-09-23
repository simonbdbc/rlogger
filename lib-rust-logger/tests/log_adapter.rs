#![cfg(feature = "log")]

use rlogger::{Config, InstanceConfig, Level, LogAdapter, Route, Runtime};
use std::sync::Arc;

#[test]
fn global_log_facade_filters_routes_flushes_and_shuts_down() {
    let temp = tempfile::tempdir().unwrap();
    let runtime = Arc::new(Runtime::new(Config::new(temp.path())).unwrap());
    let mut config = InstanceConfig::new("backend");
    config.level = Level::Info;
    config.destinations = vec!["app".into(), "audit".into()];
    config.routes = vec![Route {
        module_prefix: "routed_module".into(),
        destination: "audit".into(),
    }];
    let logger = runtime.instance(config).unwrap();

    let other = Arc::new(Runtime::new(Config::new(temp.path().join("other"))).unwrap());
    let other_logger = other.instance(InstanceConfig::new("other")).unwrap();
    assert!(LogAdapter::new(runtime.clone(), other_logger).is_err());
    other.shutdown().unwrap();

    log::set_boxed_logger(Box::new(
        LogAdapter::new(runtime.clone(), logger.clone()).unwrap(),
    ))
    .unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    assert!(!log::log_enabled!(log::Level::Debug));
    assert!(log::log_enabled!(log::Level::Info));
    log::debug!("filtered event");
    log::info!("default destination");
    log::info!(target: "audit", "audit destination");
    let routed = log::Record::builder()
        .args(format_args!("module route"))
        .level(log::Level::Info)
        .target("not-a-destination")
        .module_path_static(Some("routed_module::child"))
        .build();
    log::logger().log(&routed);
    log::logger().flush();
    assert_eq!(runtime.stats().pending, 0);
    let report = runtime.shutdown().unwrap();
    assert_eq!(report.totals.accepted, 3);
    assert_eq!(report.totals.written, 3);

    let mut app = String::new();
    let mut audit = String::new();
    for day in std::fs::read_dir(runtime.log_root()).unwrap() {
        let instance = day.unwrap().path().join("backend");
        if !instance.is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(instance).unwrap() {
            let path = entry.unwrap().path();
            let body = std::fs::read_to_string(&path).unwrap();
            if path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("audit-")
            {
                audit.push_str(&body);
            } else {
                app.push_str(&body);
            }
        }
    }
    assert!(app.contains("default destination"));
    assert!(audit.contains("audit destination"));
    assert!(audit.contains("module route"));
    assert!(!app.contains("filtered event"));
}
