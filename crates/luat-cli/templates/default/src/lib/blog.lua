-- =============================================================================
-- BLOG DATA MODULE (lib/blog.lua)
-- =============================================================================
-- This module handles all blog post operations using Luat's KV storage.
-- It demonstrates a complete blog backend with CRUD operations.
--
-- HOW TO USE:
--   local blog = require("blog")
--   local posts = blog.get_posts()          -- Get all posts (for listing)
--   local post = blog.get_post("my-slug")   -- Get single post
--   blog.create_post({ title = "Hello", content = "<p>...</p>" })
--
-- DATA STRUCTURE:
--   Each post is stored as JSON with the key pattern "post:{slug}":
--   {
--     slug: string,       -- URL-safe identifier (e.g., "my-first-post")
--     title: string,      -- Post title
--     date: string,       -- Creation date (YYYY-MM-DD)
--     updated: string,    -- Last update date (optional)
--     excerpt: string,    -- Short description for listings
--     content: string,    -- Full HTML content
--     image_url: string   -- Featured image URL
--   }
--
-- URL STRUCTURE:
--   /blog           -> List all posts (uses get_posts())
--   /blog/my-slug   -> View single post (uses get_post("my-slug"))
-- =============================================================================

-- Create a KV namespace for blog data
local kv = KV.namespace("blog")

-- Module table for exported functions
local M = {}

-- -----------------------------------------------------------------------------
-- SLUGIFY
-- -----------------------------------------------------------------------------
-- Converts a title to a URL-safe slug.
-- This is duplicated from utils.lua to keep the module self-contained.
--
-- @param text (string) - Text to convert
-- @return (string) - URL-safe slug
--
-- EXAMPLE: "Hello World!" -> "hello-world"
-- -----------------------------------------------------------------------------
function M.slugify(text)
    return text:lower():gsub("%s+", "-"):gsub("[^%w-]", "")
end

-- -----------------------------------------------------------------------------
-- GET ALL POSTS
-- -----------------------------------------------------------------------------
-- Retrieves all blog posts for listing pages.
-- Returns posts WITHOUT full content to keep responses small.
--
-- @return (table) - Array of post summaries sorted by date (newest first)
--
-- NOTE: This returns a lightweight version for listings.
--       Use get_post(slug) to get full content.
-- -----------------------------------------------------------------------------
function M.get_posts()
    local result = kv:list({ prefix = "post:" })
    local posts = {}

    for _, key in ipairs(result.keys) do
        local post = kv:get(key.name, "json")
        if post then
            -- Return only fields needed for listing (not full content)
            table.insert(posts, {
                slug = post.slug,
                title = post.title,
                date = post.date,
                excerpt = post.excerpt,
                image_url = post.image_url
            })
        end
    end

    -- Sort by date descending (newest posts first)
    table.sort(posts, function(a, b)
        return a.date > b.date
    end)

    return posts
end

-- -----------------------------------------------------------------------------
-- GET SINGLE POST
-- -----------------------------------------------------------------------------
-- Retrieves a single post by its slug, including full content.
--
-- @param slug (string) - The post's URL slug
-- @return (table|nil) - Full post object or nil if not found
-- -----------------------------------------------------------------------------
function M.get_post(slug)
    return kv:get("post:" .. slug, "json")
end

-- -----------------------------------------------------------------------------
-- CREATE POST
-- -----------------------------------------------------------------------------
-- Creates a new blog post.
--
-- @param data (table) - Post data with fields:
--   - title (required): Post title
--   - slug (optional): URL slug (auto-generated from title if not provided)
--   - date (optional): Creation date (defaults to today)
--   - excerpt (optional): Short description
--   - content (optional): Full HTML content
--   - image_url (optional): Featured image URL
--
-- @return (table|nil, string?) - New post or (nil, error message)
-- -----------------------------------------------------------------------------
function M.create_post(data)
    -- Generate slug from title if not provided
    local slug = data.slug or M.slugify(data.title)

    -- Prevent duplicate slugs
    if M.get_post(slug) then
        return nil, "A post with this slug already exists"
    end

    local post = {
        slug = slug,
        title = data.title,
        date = data.date or os.date("%Y-%m-%d"),  -- Default to today
        excerpt = data.excerpt or "",
        content = data.content or "",
        image_url = data.image_url or ""
    }

    -- Store with metadata for advanced KV features
    -- Metadata is searchable without loading the full value
    kv:put("post:" .. slug, json.encode(post), {
        metadata = {
            title = post.title,
            date = post.date
        }
    })

    return post
end

-- -----------------------------------------------------------------------------
-- UPDATE POST
-- -----------------------------------------------------------------------------
-- Updates an existing blog post.
-- The slug and original date are preserved.
--
-- @param slug (string) - The post's URL slug
-- @param data (table) - Fields to update (title, excerpt, content, image_url)
-- @return (table|nil, string?) - Updated post or (nil, error message)
--
-- NOTE: An "updated" timestamp is automatically added.
-- -----------------------------------------------------------------------------
function M.update_post(slug, data)
    local post = M.get_post(slug)
    if not post then
        return nil, "Post not found"
    end

    -- Update only provided fields (keep existing values for others)
    post.title = data.title or post.title
    post.excerpt = data.excerpt or post.excerpt
    post.content = data.content or post.content
    post.image_url = data.image_url or post.image_url
    post.updated = os.date("%Y-%m-%d")  -- Track when post was modified
    -- Don't update slug or date - these are immutable

    kv:put("post:" .. slug, json.encode(post), {
        metadata = {
            title = post.title,
            date = post.date
        }
    })

    return post
end

-- -----------------------------------------------------------------------------
-- DELETE POST
-- -----------------------------------------------------------------------------
-- Permanently removes a blog post.
--
-- @param slug (string) - The post's URL slug
-- @return (boolean, string?) - true on success, or (false, error message)
-- -----------------------------------------------------------------------------
function M.delete_post(slug)
    local post = M.get_post(slug)
    if not post then
        return false, "Post not found"
    end

    kv:delete("post:" .. slug)
    return true
end

-- -----------------------------------------------------------------------------
-- INITIALIZE SAMPLE POSTS
-- -----------------------------------------------------------------------------
-- Creates sample blog posts if the blog is empty.
-- This runs automatically when the module loads to provide demo content.
--
-- In a real app, you'd remove this and let users create their own posts.
-- -----------------------------------------------------------------------------
function M.init_sample_posts()
    -- Check if any posts exist
    local existing = kv:list({ prefix = "post:", limit = 1 })
    if #existing.keys == 0 then
        -- Create sample posts for demo purposes
        M.create_post({
            slug = "hello-world",
            title = "Hello World",
            date = "2024-01-01",
            excerpt = "Welcome to my first blog post!",
            content = "<p>Welcome to my first blog post! This is just the beginning.</p><p>We're building something <strong>amazing</strong> together.</p>",
            image_url = "https://images.unsplash.com/photo-1499750310107-5fef28a66643?w=800&q=80"
        })
        M.create_post({
            slug = "getting-started",
            title = "Getting Started with Luat",
            date = "2024-01-15",
            excerpt = "Learn how to build web apps with Luat.",
            content = "<p>Luat makes it easy to build <em>server-rendered</em> web applications with Lua.</p><ul><li>Simple routing</li><li>KV storage</li><li>HTMX integration</li></ul>",
            image_url = "https://images.unsplash.com/photo-1517694712202-14dd9538aa97?w=800&q=80"
        })
        M.create_post({
            slug = "htmx-magic",
            title = "The Magic of HTMX",
            date = "2024-02-01",
            excerpt = "Discover how HTMX brings interactivity without the JavaScript complexity.",
            content = "<p>HTMX extends HTML with powerful attributes that let you build modern, interactive web applications.</p><p>No more wrestling with complex JavaScript frameworks - just <strong>declarative attributes</strong> on your HTML elements.</p><ul><li>hx-get, hx-post for AJAX requests</li><li>hx-swap for controlling how content updates</li><li>hx-trigger for custom event handling</li></ul>",
            image_url = "https://images.unsplash.com/photo-1555066931-4365d14bab8c?w=800&q=80"
        })
        M.create_post({
            slug = "lua-for-web",
            title = "Why Lua for Web Development?",
            date = "2024-02-15",
            excerpt = "Exploring the benefits of using Lua for server-side web development.",
            content = "<p>Lua is a lightweight, fast, and embeddable scripting language that's perfect for web development.</p><p>With Luat, you get:</p><ul><li><strong>Speed</strong> - LuaJIT is incredibly fast</li><li><strong>Simplicity</strong> - Clean, readable syntax</li><li><strong>Flexibility</strong> - Easy to extend and customize</li></ul><p>It's time to rediscover the joy of simple, elegant code.</p>",
            image_url = "https://images.unsplash.com/photo-1516116216624-53e697fedbea?w=800&q=80"
        })
        M.create_post({
            slug = "building-components",
            title = "Building Reusable Components",
            date = "2024-03-01",
            excerpt = "Learn how to create and compose reusable UI components in Luat.",
            content = "<p>Components are the building blocks of modern web applications. In Luat, creating reusable components is straightforward.</p><p>Key concepts:</p><ul><li><strong>Props</strong> - Pass data to components</li><li><strong>Children</strong> - Nest content inside components</li><li><strong>Composition</strong> - Build complex UIs from simple parts</li></ul><p>Start small, think modular, and watch your productivity soar.</p>",
            image_url = "https://images.unsplash.com/photo-1558618666-fcd25c85cd64?w=800&q=80"
        })
        M.create_post({
            slug = "dark-mode-guide",
            title = "Implementing Dark Mode",
            date = "2024-03-15",
            excerpt = "A complete guide to adding dark mode support to your Luat application.",
            content = "<p>Dark mode isn't just a trend - it's an accessibility feature that many users prefer.</p><p>In this guide, we cover:</p><ul><li>Detecting user preference with <code>prefers-color-scheme</code></li><li>Storing preferences in localStorage</li><li>Toggling themes with Alpine.js</li><li>Styling with Tailwind's dark mode utilities</li></ul><p>Give your users the <em>choice</em> they deserve.</p>",
            image_url = "https://images.unsplash.com/photo-1519389950473-47ba0277781c?w=800&q=80"
        })
    end
end

-- Auto-initialize sample posts when module is first loaded
-- This ensures the blog has content to display immediately
M.init_sample_posts()

return M
