-- =============================================================================
-- BLOG API COLLECTION (routes/api/blog/+server.lua)
-- =============================================================================
-- REST API for blog posts - list all and create new.
-- Works alongside the page routes for HTMX and programmatic access.
--
-- ENDPOINTS:
--   GET  /api/blog - List all posts (JSON)
--   POST /api/blog - Create new post
--
-- USE CASES:
--   1. Build a mobile app that consumes the same blog data
--   2. Create integrations with other services
--   3. Allow programmatic content management
--
-- HTMX INTEGRATION:
--   The HX-Redirect header tells HTMX to navigate after successful
--   operations, enabling seamless web UI + API combo.
-- =============================================================================

-- Import blog module (short path works from lib_dir)
local blog = require("blog")

-- -----------------------------------------------------------------------------
-- GET /api/blog - List all posts
-- -----------------------------------------------------------------------------
-- Returns array of post summaries (without full content)
-- Response: [{ slug, title, date, excerpt, image_url }, ...]
function GET(ctx)
    return {
        status = 200,
        body = blog.get_posts()
    }
end

-- -----------------------------------------------------------------------------
-- POST /api/blog - Create new post
-- -----------------------------------------------------------------------------
-- Creates a new blog post from form data.
--
-- Required fields:
--   - title: Post title (used to generate slug)
--
-- Optional fields:
--   - excerpt: Short description
--   - content: Full HTML content
--
-- Response: The created post object
-- Also returns HX-Redirect header for HTMX clients
function POST(ctx)
    local form = ctx.form or {}

    -- Validate required fields
    if not form.title or form.title == "" then
        return {
            status = 400,
            body = { error = "Title is required" }
        }
    end

    -- Create the post
    local post, err = blog.create_post({
        title = form.title,
        excerpt = form.excerpt,
        content = form.content
    })

    -- Handle creation errors
    if not post then
        return {
            status = 400,
            body = { error = err }
        }
    end

    -- Success - return created post with redirect header
    return {
        status = 201,  -- 201 Created
        headers = {
            -- HTMX clients will automatically navigate to the new post
            ["HX-Redirect"] = "/blog/" .. post.slug
        },
        body = post
    }
end
