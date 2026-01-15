<p align="center">
  <img src="assets/luat-logo.webp" alt="luat logo" width="200">
</p>

<p align="center">
  Svelte-inspired server-side Lua templating for Rust.
</p>

<p align="center">
  <a href="https://crates.io/crates/luat"><img src="https://img.shields.io/crates/v/luat.svg" alt="Crates.io"></a>
  <a href="https://docs.rs/luat"><img src="https://docs.rs/luat/badge.svg" alt="Documentation"></a>
  <a href="https://github.com/maravilla-labs/luat/actions/workflows/ci.yml"><img src="https://github.com/maravilla-labs/luat/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="LICENSE-MIT"><img src="https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg" alt="License"></a>
</p>

<p align="center">
  <a href="https://luat.maravillalabs.com/">Website</a> •
  <a href="https://luat.maravillalabs.com/docs/getting-started">Getting Started</a> •
  <a href="https://luat.maravillalabs.com/docs/templating/syntax">Syntax Guide</a> •
  <a href="https://github.com/maravilla-labs/luat-tools">Editor Support</a>
</p>

---

> ⚠️ **Early Release** - This is the first public release of Luat. The API is still evolving and not yet production-ready. Feedback and contributions are welcome.

<p align="center">
  <img src="assets/screen-dark-sm.png" alt="Luat example app screenshot" width="800">
</p>

## Features

- **Svelte-like syntax** - Familiar `{#if}`, `{#each}`, components, and expressions
- **Server-side rendering** - Pure SSR with no client hydration overhead
- **Component system** - Reusable components with props and children
- **Lua-powered** - Templates compile to Lua for fast execution
- **Template bundling** - Bundle templates for production deployment
- **Built-in caching** - Memory or filesystem caching for compiled templates
- **CLI with live reload** - Development server with automatic browser refresh

## Installation


### From CLI

```bash
# npm (recommended for JS/TS projects)
npm install -g @maravilla-labs/luat

# Shell script (Linux/macOS)
curl -fsSL https://raw.githubusercontent.com/maravilla-labs/luat/main/scripts/install.sh | sh

# Cargo (Rust developers)
cargo install luat-cli
```

## Quick Start

### CLI Usage

```bash
# Create a new project
luat init my-app
cd my-app

# Start development server with live reload
luat dev

# Build for production
luat build
```


### Library Usage

```rust
use luat::{Engine, FileSystemResolver};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create engine with filesystem resolver
    let resolver = FileSystemResolver::new("./templates");
    let engine = Engine::with_memory_cache(resolver, 100)?;

    // Compile and render a template
    let module = engine.compile_entry("hello.luat")?;
    let context = engine.to_value(serde_json::json!({
        "name": "World",
        "items": ["Apple", "Banana", "Cherry"]
    }))?;

    let html = engine.render(&module, &context)?;
    println!("{}", html);
    Ok(())
}
```

## Template Syntax

### Text Interpolation

```html
<h1>Hello, {props.name}!</h1>
<p>Count: {props.count + 1}</p>
```

### Conditionals

```html
{#if props.user.admin}
    <p>Welcome, admin!</p>
{:else if props.user.moderator}
    <p>Welcome, moderator!</p>
{:else}
    <p>Welcome, user!</p>
{/if}
```

### Loops

```html
{#each props.items as item, index}
    <li>{index + 1}. {item.name}</li>
{:empty}
    <li>No items found</li>
{/each}
```

### Components

```html
<!-- components/Card.luat -->
<div class="card">
    <h2>{props.title}</h2>
    <div class="card-body">
        {@render props.children?.()}
    </div>
</div>
```

```html
<!-- page.luat -->
<script>
    local Card = require("components/Card")
</script>

<Card title="My Card">
    <p>Card content goes here</p>
</Card>
```

### Script Blocks

```html
<!-- Module script (runs once per module) -->
<script module>
    function formatPrice(price)
        return string.format("$%.2f", price)
    end
</script>

<!-- Regular script (runs on each render) -->
<script>
    local Card = require("Card")
    local formatted = formatPrice(props.price)
</script>
```

### Raw HTML

```html
<!-- Render unescaped HTML (use with caution) -->
<div>{@html props.content}</div>
```

## Project Structure

When using the CLI, the recommended project structure is:

```
my-app/
├── luat.toml           # Project configuration
├── templates/          # Template files
│   ├── index.luat
│   └── components/
│       └── Card.luat
├── public/             # Static assets
└── dist/               # Build output
```

### Configuration (luat.toml)

```toml
[project]
name = "my-app"
version = "0.1.0"

[dev]
port = 3000
host = "127.0.0.1"
templates_dir = "templates"
public_dir = "public"

[build]
output_dir = "dist"
bundle_format = "source"  # or "binary"
```

## Editor Support

Get syntax highlighting, diagnostics, and autocomplete for `.luat` files:

- **VSCode Extension** - Full language support with LSP integration
- **LSP Server** - Works with any LSP-compatible editor (Neovim, Helix, etc.)

See [luat-tools](https://github.com/maravilla-labs/luat-tools) for installation instructions.

## Documentation

- [Getting Started](docs/getting-started.md)
- [Template Syntax](docs/template-syntax.md)
- [CLI Reference](docs/cli-reference.md)
- [API Documentation](https://docs.rs/luat)

## Examples

See the [examples](examples/) directory for complete working examples:

- **basic** - Simple library usage with memory and filesystem resolvers

## Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Credits

Created and maintained by [Maravilla Labs](https://maravilla-labs.com).
