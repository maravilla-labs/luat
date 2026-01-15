-- =============================================================================
-- NEW POST SERVER (routes/blog/new/+page.server.lua)
-- =============================================================================
-- Server-side logic for creating new blog posts.
-- Demonstrates form actions and HTMX redirect responses.
--
-- ACTIONS:
--   Actions handle form submissions (POST requests).
--   The action name comes from the URL query: /blog/new?/default
--   calls the "default" action.
--
-- FORM DATA:
--   ctx.form contains the submitted form fields:
--   - title: Post title (required)
--   - excerpt: Short description
--   - content: HTML content from rich text editor
--   - image_url: Cover image URL
--
-- HTMX REDIRECT:
--   After successful creation, we return an HX-Redirect header.
--   HTMX intercepts this and navigates to the new post page.
-- =============================================================================

-- Import the blog module for data operations
local blog = require("../../../lib/blog")

-- Load function returns initial page data
function load(ctx)
    return {
        title = "New Post"  -- Browser tab title
    }
end

-- Actions table contains form submission handlers
-- Each key is an action name that can be called via ?/actionName
actions = {
    -- The "default" action handles the main form submission
    -- Called via POST /blog/new?/default
    default = function(ctx)
        local form = ctx.form or {}

        -- VALIDATION
        -- Check required fields before processing
        if not form.title or form.title == "" then
            -- Return error with fail() helper
            -- This returns a 400 status and preserves form data
            return fail(400, {
                error = "Title is required",
                title = form.title,
                excerpt = form.excerpt,
                content = form.content
            })
        end

        -- CREATE THE POST
        -- Call blog module to save the new post
        local post, err = blog.create_post({
            title = form.title,
            excerpt = form.excerpt,
            content = form.content,
            image_url = form.image_url
        })

        -- Handle creation errors (e.g., duplicate slug)
        if not post then
            return fail(400, {
                error = err,
                title = form.title,
                excerpt = form.excerpt,
                content = form.content
            })
        end

        -- SUCCESS - REDIRECT TO NEW POST
        -- Return data with HX-Redirect header for HTMX navigation
        return {
            success = true,
            post = post,
            headers = {
                -- HTMX will intercept this header and navigate to the URL
                ["HX-Redirect"] = "/blog/" .. post.slug
            }
        }
    end
}
