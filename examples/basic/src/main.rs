// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

// Allow some clippy lints for this example
#![allow(clippy::needless_borrows_for_generic_args)]

use luat::memory_resolver::MemoryResourceResolver;
use luat::{Engine, FileSystemResolver};
use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::path::Path;

fn main() -> Result<(), Box<dyn Error>> {
    // This example demonstrates two approaches:
    // 1. Using the MemoryResourceResolver (in-memory templates)
    // 2. Using the FileSystemResolver (templates from filesystem)

    println!("=== Memory Resolver Example ===");
    memory_example()?;

    println!("\n=== Filesystem Resolver Example ===");
    filesystem_example()?;

    Ok(())
}

fn memory_example() -> Result<(), Box<dyn Error>> {
    // Create a memory resolver
    let resolver = MemoryResourceResolver::new();
    
    // Add template components to the resolver
    resolver.add_template("Layout.luat", r#"
<!DOCTYPE html>
<html>
<head>
    <title>{props.title}</title>
    <style>
        .card { border: 1px solid #eee; border-radius: 8px; padding: 16px; margin-bottom: 16px; }
        .card h2 { margin-top: 0; }
        .button { background: #4B5563; color: white; border: 0; padding: 8px 16px; border-radius: 4px; }
    </style>
</head>
<body>
    <h1>{props.title}</h1>
    {@render props.children?.()}
</body>
</html>"#.to_string());

    resolver.add_template("Card.luat", r#"
<div class="card">
    <h2>{props.title}</h2>
    <div class="card-body">
        {@render props.children?.()}
    </div>
</div>"#.to_string());

    resolver.add_template("Button.luat", r#"
<button class="button" onclick="{props.onClick}">
    {@render props.children?.()}
</button>"#.to_string());

    // Create our main app template
    resolver.add_template("App.luat", r#"
<script>
    local Layout = require("Layout")
    local Card = require("Card")
    local Button = require("Button")
    
    local count = 0
    local function increment()
        count = count + 1
        return count
    end
</script>

<Layout title="Dashboard">
    <Card title="User Profile">
        <p>Name: {props.user.name}</p>
        <p>Email: {props.user.email}</p>
        
        {#if props.user.admin}
            <p><strong>Admin User</strong></p>
        {/if}
        
        <Button onClick="alert('Hello, {props.user.name}!')">
            Greet User
        </Button>
    </Card>

    <Card title="Activity">
        {#each props.activities as activity, i}
            <div>
                <strong>{i+1}.</strong> {activity.description} - {activity.date}
            </div>
        {/each}
    </Card>
    
    <p>Generated at: {os.date()}</p>
</Layout>"#.to_string());

    // Create the engine with our resolver
    let engine = Engine::with_memory_cache(resolver, 100)?;
    
    // Create data to pass to the template
    let mut context: HashMap<String, mlua::Value> = HashMap::new();
    
    // Create user object with updated API
    let mut user_data: HashMap<String, mlua::Value> = HashMap::new();
    user_data.insert("name".to_string(), engine.create_string("Alice Smith")?);
    user_data.insert("email".to_string(), engine.create_string("alice@example.com")?);
    user_data.insert("admin".to_string(), engine.create_boolean(true)?);
    
    let user_table = engine.create_table_from_hashmap(user_data)?;
    
    context.insert("user".to_string(), mlua::Value::Table(user_table));
    
    // Create activities array
    let activities_data: Vec<HashMap<String, mlua::Value>> = vec![
        {
            let mut activity: HashMap<String, mlua::Value> = HashMap::new();
            activity.insert("description".to_string(), engine.create_string("Logged in")?);
            activity.insert("date".to_string(), engine.create_string("2025-06-15")?);
            activity
        },
        {
            let mut activity: HashMap<String, mlua::Value> = HashMap::new();
            activity.insert("description".to_string(), engine.create_string("Updated profile")?);
            activity.insert("date".to_string(), engine.create_string("2025-06-14")?);
            activity
        },
        {
            let mut activity: HashMap<String, mlua::Value> = HashMap::new();
            activity.insert("description".to_string(), engine.create_string("Created account")?);
            activity.insert("date".to_string(), engine.create_string("2025-06-13")?);
            activity
        },
    ];
    
    let activities_table = engine.create_table_from_vec(activities_data)?;
    
    context.insert("activities".to_string(), mlua::Value::Table(activities_table));
    
    // Compile and render the template
    let result = engine.render_source("App.luat", &context)?;
    
    println!("{}", result);
    
    Ok(())
}

fn filesystem_example() -> Result<(), Box<dyn Error>> {
    // Create a temporary directory for our templates
    // Create a persistent directory for templates
    let examples_dir = Path::new("examples/temp");
    fs::create_dir_all(&examples_dir)?;
    let templates_dir = examples_dir.join("templates");
    fs::create_dir_all(&templates_dir)?;
    let ui_dir = templates_dir.join("ui");
    fs::create_dir_all(&ui_dir)?;
    
    println!("Template directory created at: {}", templates_dir.display());
    println!("NOTE: Please delete the 'examples/temp' directory when done with the example.");
    println!();
    println!("Template directory: {}", templates_dir.display());
    
    // Create component files
    fs::write(
        templates_dir.join("ui/Button.luat"),
        r#"<button class="button">{@render props.children?.()}</button>"#
    )?;
    
    fs::write(
        templates_dir.join("Card.luat"),
        r#"
<script module>
    function getCardClass(variant)
        return "card card-" .. (variant or "default")
    end
</script>

<script>
    local variant = props.variant or "default"
    local class = getCardClass(variant)
</script>

<div class={class}>
    <h2>{props.title}</h2>
    <div class="content">
        {@render props.children?.()}
    </div>
</div>"#
    )?;
    let mainscript =         r#"
<script>
    local Card = require("Card")
    local Button = require("./ui/Button")
</script>


<div class="container">
    <Card variant="primary" title="Welcome">
        <p>This is a simple example of using components.</p>
        
        <Button>Click me</Button>
    </Card>
    
    <Card variant="secondary" title="Features">
        <ul>
            {#each props.features as feature}
                <li>{feature}</li>
            {/each}
        </ul>
    </Card>

    <hr/>

</div>"#;

    // Create main template
    fs::write(
        templates_dir.join("main.luat"),
        mainscript
    )?;
    
    // Create the engine with filesystem resolver
    let resolver = FileSystemResolver::new(templates_dir);
    let engine = Engine::with_memory_cache(resolver, 100)?;
    
    // Prepare data for rendering
    // let mut context = HashMap::new();
    
    // Create features array
    let features_data = vec![
        "Easy to use".to_string(),
        "High performance".to_string(),
        "Modular components".to_string(),
        "Caching built-in".to_string(),
    ];
    
    // let features_table = engine.create_table_from_vec(features_data)?;
    
    // context.insert("features".to_string(), features_table);
    

    let nucontext = engine.to_value(features_data)?;

    // nucontext.set_value("features", features)?;
        

    let luaresult = engine.compile_template_string("name",mainscript);
    println!("Lua result: {:?}", luaresult);

    // Compile and render
    let module = engine.compile_entry("main.luat")?;
    let result = engine.render(&module, &nucontext)?;
    // Save the result to a file
    let result_path = examples_dir.join("result.html");
    fs::write(&result_path, &result)?;
    
    println!("\nResult saved to: {}", result_path.display());
    println!("Full path: {}", result_path.canonicalize()?.display());
    
    // println!("{}", result);
    
    Ok(())
}
