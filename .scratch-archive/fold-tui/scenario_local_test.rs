// ABOUTME: Quick local gateway test
// ABOUTME: Tests against localhost:50051

use fold_client::FoldClient;

fn main() {
    println!("Testing LOCAL gateway at localhost:50051...\n");
    
    let ssh_key = dirs::home_dir().unwrap().join(".config/fold/agent_key");
    
    let client = match FoldClient::new_with_auth("http://localhost:50051".to_string(), &ssh_key) {
        Ok(c) => c,
        Err(e) => {
            println!("❌ Failed: {:?}", e);
            return;
        }
    };
    
    println!("Health check...");
    if let Err(e) = client.check_health() {
        println!("❌ Health failed: {:?}", e);
        return;
    }
    println!("✅ Connected\n");
    
    println!("Listing agents...");
    match client.refresh_agents() {
        Ok(agents) => {
            println!("Found {} agent(s):", agents.len());
            for a in &agents {
                println!("  {} {} - {} ({})", 
                    if a.connected { "🟢" } else { "🔴" },
                    a.name, a.backend, a.working_dir);
            }
        }
        Err(e) => println!("❌ Failed: {:?}", e),
    }
}
