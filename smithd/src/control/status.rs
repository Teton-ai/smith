use crate::dbus::SmithDbusProxy;
use crate::magic::MagicHandle;
use crate::shutdown::ShutdownHandler;
use anyhow::Result;
use tracing::info;
use zbus::Connection;

pub async fn status() -> Result<()> {
    let mut exit_code = 0;

    let connection = Connection::system().await?;

    let proxy = SmithDbusProxy::new(&connection).await?;

    let reply = proxy.updater_status().await?;

    println!("{reply}");

    info!("Checking installed packages");

    let shutdown = ShutdownHandler::new();

    let configuration = MagicHandle::new(shutdown.signals());

    configuration.load(None).await;

    let magic_packages = configuration.get_packages().await;

    // check the system version of the packages in the magic file
    for package in magic_packages {
        let installed_version = match package.get_system_version().await {
            Ok(v) => v,
            Err(e) => {
                println!(
                    "{}: Failed to get system version, magic.toml version is {}. Error: {}",
                    package.name, package.version, e
                );
                continue;
            }
        };
        let magic_toml_version = package.version;

        println!(
            "{}: {} | {} | {}",
            package.name,
            magic_toml_version,
            installed_version,
            magic_toml_version == installed_version
        );

        if magic_toml_version != installed_version {
            exit_code = -1;
        }
    }

    std::process::exit(exit_code);
}
