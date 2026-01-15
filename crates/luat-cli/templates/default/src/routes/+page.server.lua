-- =============================================================================
-- HOME PAGE SERVER (+page.server.lua)
-- =============================================================================
-- This file handles server-side logic for the home page (/).
--
-- LOAD FUNCTION:
--   The load() function runs on every GET request to this page.
--   Data returned here becomes available as "props" in +page.luat.
--
-- WHAT YOU CAN DO HERE:
--   - Fetch data from databases or APIs
--   - Read from KV storage
--   - Process query parameters
--   - Set response headers
--   - Redirect to other pages
--
-- EXAMPLE USE CASES:
--   - Load user data: return { user = kv.get("users", userId) }
--   - Fetch posts: return { posts = kv.get("blog", "posts") or {} }
--   - Pass config: return { apiUrl = os.getenv("API_URL") }
-- =============================================================================

-- Load function runs on every request to this page
-- The ctx parameter contains request context (url, method, params, etc.)
-- Return data that will be passed to the template as props
function load(ctx)
    return {
        title = "Home",                    -- Sets the page title (used in app.html)
        message = "Hello from the server!" -- Custom data passed to the template
    }
end
