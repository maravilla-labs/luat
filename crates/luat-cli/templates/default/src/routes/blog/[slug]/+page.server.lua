-- =============================================================================
-- BLOG POST DETAIL SERVER (routes/blog/[slug]/+page.server.lua)
-- =============================================================================
-- Server-side logic for displaying a single blog post.
-- Demonstrates dynamic route parameters and 404 handling.
--
-- DYNAMIC ROUTES:
--   The [slug] directory creates a dynamic segment.
--   When user visits /blog/hello-world:
--   - ctx.params.slug = "hello-world"
--
-- 404 HANDLING:
--   If the post doesn't exist, we return status = 404.
--   Luat will use the returned data to render an error state.
--
-- REQUIRE PATH:
--   require("lib/blog") - Can use short path because lib_dir is configured
-- =============================================================================

-- Import the blog module
local blog = require("lib/blog")

-- Load function runs on every GET request
function load(ctx)
    -- Extract the slug from URL parameters
    -- e.g., /blog/hello-world -> slug = "hello-world"
    local slug = ctx.params.slug

    -- Fetch the post by its slug
    local post = blog.get_post(slug)

    -- HANDLE NOT FOUND
    -- If post doesn't exist, return 404 status with error message
    if not post then
        return {
            status = 404,  -- HTTP status code
            title = "Not Found",
            post = {
                title = "Post Not Found",
                date = "",
                content = "The requested blog post could not be found."
            }
        }
    end

    -- SUCCESS - Return the full post data
    return {
        title = post.title,  -- Browser tab shows post title
        post = post          -- Full post object for template
    }
end
