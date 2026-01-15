-- =============================================================================
-- UTILITY FUNCTIONS (lib/utils.lua)
-- =============================================================================
-- Shared utility functions that can be used across your application.
-- This module demonstrates how to create reusable Lua modules in Luat.
--
-- HOW TO USE:
--   In any +page.server.lua or +server.lua file:
--   local utils = require("utils")
--   local date = utils.format_date(os.time())
--   local slug = utils.slugify("My Blog Post Title")
--
-- MODULE PATTERN:
--   Lua modules use a table (M) to export functions.
--   The table is returned at the end so other files can use it.
--
-- FILES ARE AUTO-DISCOVERED:
--   Files in lib_dir (src/lib/) can be required without path prefix.
--   So require("utils") finds src/lib/utils.lua automatically.
-- =============================================================================

-- Create a module table to hold our exported functions
local M = {}

-- -----------------------------------------------------------------------------
-- FORMAT DATE
-- -----------------------------------------------------------------------------
-- Converts a Unix timestamp to a human-readable date string.
-- Unix timestamps are seconds since January 1, 1970.
--
-- @param timestamp (number) - Unix timestamp (e.g., from os.time())
-- @return (string) - Formatted date like "2024-01-15"
--
-- EXAMPLE:
--   utils.format_date(1705276800) --> "2024-01-15"
--   utils.format_date(os.time())  --> Today's date
-- -----------------------------------------------------------------------------
function M.format_date(timestamp)
    return os.date("%Y-%m-%d", timestamp)
end

-- -----------------------------------------------------------------------------
-- SLUGIFY
-- -----------------------------------------------------------------------------
-- Converts text to a URL-safe "slug" for use in URLs.
-- Used for creating SEO-friendly blog post URLs.
--
-- @param text (string) - Any text to convert
-- @return (string) - Lowercase, hyphenated, alphanumeric string
--
-- TRANSFORMATIONS:
--   1. Convert to lowercase
--   2. Replace spaces with hyphens
--   3. Remove non-alphanumeric characters (except hyphens)
--
-- EXAMPLES:
--   utils.slugify("Hello World!")     --> "hello-world"
--   utils.slugify("My Blog Post #1")  --> "my-blog-post-1"
--   utils.slugify("Café & Restaurant") --> "caf-restaurant"
-- -----------------------------------------------------------------------------
function M.slugify(text)
    return text:lower():gsub("%s+", "-"):gsub("[^%w-]", "")
end

-- Return the module table so other files can use these functions
return M
