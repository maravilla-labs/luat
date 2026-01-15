-- Shared utility functions
-- Import in your +page.server.lua files with: local utils = require("utils")

local M = {}

function M.format_date(timestamp)
    return os.date("%Y-%m-%d", timestamp)
end

function M.slugify(text)
    return text:lower():gsub("%s+", "-"):gsub("[^%w-]", "")
end

function M.truncate(text, length)
    if #text <= length then
        return text
    end
    return text:sub(1, length) .. "..."
end

return M
