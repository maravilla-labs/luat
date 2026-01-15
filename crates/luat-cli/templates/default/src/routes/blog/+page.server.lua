-- =============================================================================
-- BLOG LISTING SERVER (routes/blog/+page.server.lua)
-- =============================================================================
-- Server-side logic for the blog listing page.
-- Fetches all blog posts for display in the listing.
--
-- REQUIRE PATH:
--   require("../../lib/blog") - Go up two directories from routes/blog/
--   to reach src/lib/blog.lua
--
-- WHAT GETS RETURNED:
--   - title: Page title for browser tab
--   - posts: Array of post objects (without full content)
--
-- DATA OPTIMIZATION:
--   blog.get_posts() returns post summaries (slug, title, date, excerpt,
--   image_url) without the full content. This keeps the listing page fast.
-- =============================================================================

-- Import the blog module for data access
local blog = require("../../lib/blog")

-- Load function runs on every GET request to /blog
function load(ctx)
    -- Fetch all posts (returns summaries, sorted by date)
    local posts = blog.get_posts()

    return {
        title = "Blog",  -- Browser tab title
        posts = posts    -- Array passed to template as props.posts
    }
end
