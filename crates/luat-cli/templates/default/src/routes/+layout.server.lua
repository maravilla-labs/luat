-- =============================================================================
-- ROOT LAYOUT SERVER (+layout.server.lua)
-- =============================================================================
-- This file provides data to the root layout template (+layout.luat).
-- The load() function runs on EVERY page request because this is the root layout.
--
-- HOW DATA FLOWS:
--   1. User requests /blog
--   2. Luat calls this load() function
--   3. Returned data becomes available as "props" in +layout.luat
--   4. Child page's load() data is also available in +page.luat
--
-- CONTEXT OBJECT (ctx):
--   ctx.url      - The current request URL path (e.g., "/blog")
--   ctx.method   - HTTP method (GET, POST, etc.)
--   ctx.params   - URL parameters for dynamic routes
--   ctx.query    - Query string parameters
--   ctx.headers  - Request headers
--   ctx.cookies  - Request cookies
--   ctx.form     - Form data (for POST requests)
-- =============================================================================

-- The load function runs for all pages to provide layout data
-- Here we pass the URL so the navigation can highlight the active page
function load(ctx)
    return {
        url = ctx.url  -- Pass current URL to layout for navigation highlighting
    }
end
