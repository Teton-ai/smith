/// Simple test to verify the test environment can start
/// This tests Docker container orchestration without requiring full API/device integration

mod common;

#[tokio::test]
#[ignore]
async fn test_environment_starts() {
    eprintln!("Starting test environment...");
    
    let env = common::SmithTestEnvironment::start()
        .await
        .expect("Failed to start test environment");
    
    eprintln!("Test environment started successfully!");
    
    // Get URLs
    let api_url = env.api_base_url().await.expect("Failed to get API URL");
    eprintln!("API URL: {}", api_url);
    
    let postgres_url = env.postgres_url().await.expect("Failed to get Postgres URL");
    eprintln!("Postgres URL: {}", postgres_url);
    
    // Try to connect to postgres
    eprintln!("Testing postgres connection...");
    let result = reqwest::Client::new()
        .get(&format!("{}/_health", api_url))
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await;
    
    match result {
        Ok(resp) => eprintln!("API responded with status: {:?}", resp.status()),
        Err(e) => eprintln!("API not responding (expected if API image not ready): {}", e),
    }
    
    eprintln!("Environment test complete!");
}

#[tokio::test]
#[ignore]
async fn test_api_container_health() {
    eprintln!("Testing API container...");
    
    let env = common::SmithTestEnvironment::start()
        .await
        .expect("Failed to start test environment");
    
    let api_url = env.api_base_url().await.expect("Failed to get API URL");
    eprintln!("API URL: {}", api_url);
    
    // Wait a bit for API to be ready
    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    
    // Try to hit the API
    let client = reqwest::Client::new();
    
    for i in 1..=5 {
        eprintln!("Attempt {} to connect to API...", i);
        
        match client.get(&api_url).timeout(std::time::Duration::from_secs(5)).send().await {
            Ok(resp) => {
                eprintln!("✓ API responded! Status: {}", resp.status());
                return;
            }
            Err(e) => {
                eprintln!("✗ API connection failed: {}", e);
                if i < 5 {
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
        }
    }
    
    panic!("API never became healthy");
}

#[tokio::test]
#[ignore]
async fn test_device_container_starts() {
    eprintln!("Testing device container startup...");
    
    let env = common::SmithTestEnvironment::start()
        .await
        .expect("Failed to start test environment");
    
    let config = common::DeviceConfig::default();
    eprintln!("Spawning device with serial: {}", config.serial_number);
    
    let device = env
        .spawn_device(config.clone())
        .await
        .expect("Failed to spawn device");
    
    eprintln!("✓ Device container started successfully!");
    
    // Give it time to initialize
    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    
    eprintln!("Device serial: {}", device.serial_number);
    eprintln!("Test complete!");
    
    device.stop().await.expect("Failed to stop device");
}
