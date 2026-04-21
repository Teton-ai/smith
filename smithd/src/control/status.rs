use crate::dbus::SmithDbusProxy;
use crate::magic::MagicHandle;
use crate::magic::structure::ConfigPackage;
use crate::shutdown::ShutdownHandler;
use anyhow::Result;
use tracing::info;
use zbus::Connection;
use anyhow::Context;

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

    let target_release_id = configuration
        .get_target_release_id()
        .await
        .with_context(|| "Failed to get Target Release ID")?;

    //if this unwrap fails, there's no point continuing
    let smith_home = std::env::current_dir().unwrap();
    let packages_dir = smith_home.join("packages");
    let release_cache = packages_dir.join("versions").join(target_release_id.to_string());

    // read the file from release cache
    let content = tokio::fs::read(&release_cache).await?;
    let content = std::str::from_utf8(&content)?;

    let packages: Vec<ConfigPackage> = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let mut parts = line.splitn(3, ' ');
            Ok::<_, anyhow::Error>(ConfigPackage {
                name: parts.next().ok_or_else(|| anyhow::anyhow!("missing name"))?.to_string(),
                version: parts.next().ok_or_else(|| anyhow::anyhow!("missing version"))?.to_string(),
                file: parts.next().ok_or_else(|| anyhow::anyhow!("missing file"))?.to_string(),
            })
        })
        .collect::<Result<_, _>>()?;

    // check the system version of the packages in the magic file
    for package in packages {
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
