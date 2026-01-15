# Luat Routing Example with Tailwind CSS

This example demonstrates SvelteKit-style file-based routing in Luat with Tailwind CSS for styling.

## Running the Example

```bash
cd examples/routing
luat dev
```

The first run will download Tailwind CSS automatically. Then visit:
- http://localhost:3000 - Home page
- http://localhost:3000/about - Static page
- http://localhost:3000/blog - Blog list
- http://localhost:3000/blog/hello-world - Dynamic route
- http://localhost:3000/api/hello - API endpoint (JSON)

## Project Structure

```
routing/
├── luat.toml                    # Project config (Tailwind enabled)
├── src/
│   ├── app.html                 # HTML shell (DOCTYPE, head, body)
│   ├── routes/
│   │   ├── +layout.luat         # Root layout (nav, main, footer)
│   │   ├── +page.luat           # Home page (/)
│   │   ├── +page.server.lua     # Home page data loading
│   │   ├── about/
│   │   │   └── +page.luat       # About page (/about)
│   │   ├── blog/
│   │   │   ├── +page.luat       # Blog list (/blog)
│   │   │   ├── +page.server.lua # Load blog posts
│   │   │   └── [slug]/          # Dynamic route
│   │   │       ├── +page.luat   # Blog post (/blog/:slug)
│   │   │       └── +page.server.lua
│   │   └── api/
│   │       └── hello/
│   │           └── +server.lua  # API endpoint (/api/hello)
│   └── lib/
│       └── utils.lua            # Shared Lua modules
├── static/                      # Static assets
└── public/
    └── css/
        └── tailwind.css         # Generated Tailwind CSS
```

## Frontend Toolchain

This example uses the Luat frontend toolchain with Tailwind CSS:

```toml
[frontend]
enabled = ["tailwind"]
tailwind_version = "4.0.5"
tailwind_output = "public/css/tailwind.css"
tailwind_content = ["src/**/*.luat", "src/**/*.html"]
```

Tailwind CSS is automatically:
- Downloaded on first run
- Compiled with content scanning from `.luat` and `.html` files
- Watched for changes in dev mode
- Minified for production builds

## Key Concepts

### app.html - HTML Shell
The `src/app.html` file provides the HTML document structure with placeholders:
- `%luat.title%` - Page title from props
- `%luat.head%` - CSS/JS links (Tailwind CSS auto-injected)
- `%luat.body%` - Rendered page content

### +layout.luat - Layouts
Layouts wrap pages using Tailwind classes for styling:
```html
<nav class="bg-slate-900 px-8 py-4 flex gap-6">
    <a href="/" class="text-white hover:text-blue-400">Home</a>
</nav>
<main class="max-w-3xl mx-auto px-4 py-8">
    {@html props.children}
</main>
```

### +page.server.lua - Data Loading
Server-side functions that return data for templates:
```lua
function load(ctx)
    return {
        title = "Page Title",
        data = fetch_data()
    }
end
```

### +server.lua - API Routes
JSON API endpoints with HTTP method handlers:
```lua
function GET(ctx)
    return { status = 200, body = { message = "Hello" } }
end
```

### Dynamic Routes
Use `[param]` syntax for URL parameters:
- `[slug]` - Required parameter (access via `ctx.params.slug`)
- `[[optional]]` - Optional parameter
- `[...rest]` - Catch-all parameter
