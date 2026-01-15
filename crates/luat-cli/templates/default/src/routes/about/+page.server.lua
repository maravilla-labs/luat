-- =============================================================================
-- ABOUT PAGE SERVER (routes/about/+page.server.lua)
-- =============================================================================
-- Server-side logic for the About page.
--
-- STATIC PAGES:
--   The About page is mostly static content. The load function only needs
--   to return the page title. All other content is in the template.
--
-- WHEN TO USE +page.server.lua:
--   - When you need to fetch data from databases or APIs
--   - When you need to process query parameters or form data
--   - When you need to set custom response headers
--
-- WHEN YOU CAN SKIP IT:
--   - For purely static pages with no dynamic data
--   - But you still need it to set the page title
-- =============================================================================

-- Load function returns data for the template
function load(ctx)
    return {
        title = "About"  -- Sets the page title in the browser tab
    }
end
