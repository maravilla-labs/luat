-- =============================================================================
-- BLOG POST API (routes/api/blog/[slug]/+server.lua)
-- =============================================================================
-- REST API for individual blog posts - get, update, delete.
-- Demonstrates dynamic API routes with [slug] parameter.
--
-- ENDPOINTS:
--   GET    /api/blog/:slug - Get single post
--   POST   /api/blog/:slug - Update post
--   PUT    /api/blog/:slug - Update post (REST alias)
--   DELETE /api/blog/:slug - Delete post
--
-- DYNAMIC ROUTE:
--   The [slug] directory name creates a dynamic segment.
--   ctx.params.slug contains the actual value from the URL.
--   Example: /api/blog/hello-world -> ctx.params.slug = "hello-world"
--
-- METHOD ALIASING:
--   PUT = POST assigns the same handler to both methods.
--   This supports both traditional REST (PUT) and HTML forms (POST).
-- =============================================================================

-- Import blog module
local blog = require("blog")

-- -----------------------------------------------------------------------------
-- GET /api/blog/:slug - Get single post
-- -----------------------------------------------------------------------------
-- Returns full post data including content.
--
-- Response: { slug, title, date, updated, excerpt, content, image_url }
-- Error: 404 if post not found
function GET(ctx)
    local slug = ctx.params.slug
    local post = blog.get_post(slug)

    if not post then
        return {
            status = 404,
            body = { error = "Post not found" }
        }
    end

    return {
        status = 200,
        body = post
    }
end

-- -----------------------------------------------------------------------------
-- POST /api/blog/:slug - Update post
-- -----------------------------------------------------------------------------
-- Updates an existing post's content.
-- The slug cannot be changed (immutable identifier).
--
-- Updateable fields:
--   - title: Post title
--   - excerpt: Short description
--   - content: Full HTML content
--
-- Response: Updated post object
-- Error: 404 if post not found, 400 if validation fails
function POST(ctx)
    local slug = ctx.params.slug
    local form = ctx.form or {}

    -- Validate required fields
    if not form.title or form.title == "" then
        return {
            status = 400,
            body = { error = "Title is required" }
        }
    end

    -- Update the post
    local post, err = blog.update_post(slug, {
        title = form.title,
        excerpt = form.excerpt,
        content = form.content
    })

    if not post then
        return {
            status = 404,
            body = { error = err }
        }
    end

    -- Return updated post with redirect header for HTMX
    return {
        status = 200,
        headers = {
            ["HX-Redirect"] = "/blog/" .. slug
        },
        body = post
    }
end

-- Also support PUT for REST-style updates
-- This allows: PUT /api/blog/hello-world
PUT = POST

-- -----------------------------------------------------------------------------
-- DELETE /api/blog/:slug - Delete post
-- -----------------------------------------------------------------------------
-- Permanently removes a blog post.
--
-- Response: { success: true }
-- Error: 404 if post not found
function DELETE(ctx)
    local slug = ctx.params.slug
    local success, err = blog.delete_post(slug)

    if not success then
        return {
            status = 404,
            body = { error = err }
        }
    end

    -- Return success with redirect to blog list for HTMX
    return {
        status = 200,
        headers = {
            ["HX-Redirect"] = "/blog"
        },
        body = { success = true }
    }
end
