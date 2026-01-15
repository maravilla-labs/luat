-- =============================================================================
-- TODO DATA MODULE (lib/todos.lua)
-- =============================================================================
-- This module handles all todo data operations using Luat's KV (Key-Value) storage.
-- It demonstrates a complete CRUD pattern (Create, Read, Update, Delete).
--
-- HOW TO USE:
--   local todos = require("todos")
--   local all_todos = todos.get_todos("all")
--   local new_todo = todos.add_todo("Buy groceries")
--   todos.toggle_todo(new_todo.id)
--
-- KV STORAGE OVERVIEW:
--   Luat provides built-in KV storage - a simple key-value database.
--   - KV.namespace("todos"): Creates/accesses a namespace for todos
--   - kv:put(key, value): Store data
--   - kv:get(key, "json"): Retrieve and parse JSON data
--   - kv:list({prefix = "..."): List all keys with a prefix
--   - kv:delete(key): Remove data
--
-- DATA STRUCTURE:
--   Each todo is stored as JSON with the key pattern "todo:{id}":
--   {
--     id: string,        -- Unique identifier (timestamp-random)
--     text: string,      -- The todo text
--     completed: bool,   -- Whether it's done
--     created: number    -- Unix timestamp when created
--   }
-- =============================================================================

-- Create a KV namespace for todos
-- Namespaces keep data organized and separate from other app data
local kv = KV.namespace("todos")

-- Module table for exported functions
local M = {}

-- -----------------------------------------------------------------------------
-- GENERATE UNIQUE ID
-- -----------------------------------------------------------------------------
-- Creates a unique identifier for new todos by combining:
-- - Current timestamp (seconds since 1970)
-- - Random 4-digit number (for uniqueness within same second)
--
-- This is a simple approach. Production apps might use UUIDs.
-- -----------------------------------------------------------------------------
local function generate_id()
    return tostring(os.time()) .. "-" .. tostring(math.random(1000, 9999))
end

-- -----------------------------------------------------------------------------
-- GET TODOS
-- -----------------------------------------------------------------------------
-- Retrieves todos with optional filtering.
--
-- @param filter (string) - "all", "active", or "completed"
-- @return (table) - Array of todo objects sorted by creation date
--
-- HOW IT WORKS:
--   1. List all keys starting with "todo:" from KV storage
--   2. Fetch each todo by its key
--   3. Apply filter (all/active/completed)
--   4. Sort by created timestamp (oldest first)
-- -----------------------------------------------------------------------------
function M.get_todos(filter)
    filter = filter or "all"

    -- List all keys with prefix "todo:" - returns { keys = [{name = "todo:123"}, ...] }
    local result = kv:list({ prefix = "todo:" })
    local todos = {}

    -- Fetch each todo and apply filter
    for _, key in ipairs(result.keys) do
        local todo = kv:get(key.name, "json")  -- "json" auto-parses the stored JSON
        if todo then
            local include = false
            if filter == "all" then
                include = true
            elseif filter == "active" and not todo.completed then
                include = true
            elseif filter == "completed" and todo.completed then
                include = true
            end

            if include then
                table.insert(todos, todo)
            end
        end
    end

    -- Sort by created date (oldest first for natural list order)
    table.sort(todos, function(a, b)
        return a.created < b.created
    end)

    return todos
end

-- -----------------------------------------------------------------------------
-- GET SINGLE TODO
-- -----------------------------------------------------------------------------
-- Retrieves a single todo by its ID.
--
-- @param id (string) - The todo's unique identifier
-- @return (table|nil) - The todo object or nil if not found
-- -----------------------------------------------------------------------------
function M.get_todo(id)
    return kv:get("todo:" .. id, "json")
end

-- -----------------------------------------------------------------------------
-- ADD TODO
-- -----------------------------------------------------------------------------
-- Creates a new todo with the given text.
--
-- @param text (string) - The todo text
-- @return (table) - The newly created todo object
--
-- The new todo starts as not completed and gets a unique ID.
-- -----------------------------------------------------------------------------
function M.add_todo(text)
    local id = generate_id()
    local todo = {
        id = id,
        text = text,
        completed = false,
        created = os.time()  -- Current Unix timestamp
    }

    -- Store as JSON string in KV
    kv:put("todo:" .. id, json.encode(todo))
    return todo
end

-- -----------------------------------------------------------------------------
-- TOGGLE TODO
-- -----------------------------------------------------------------------------
-- Toggles the completed state of a todo (done <-> not done).
--
-- @param id (string) - The todo's ID
-- @return (table|nil, string?) - Updated todo or (nil, error message)
-- -----------------------------------------------------------------------------
function M.toggle_todo(id)
    local todo = M.get_todo(id)
    if not todo then
        return nil, "Todo not found"
    end

    -- Flip the completed state
    todo.completed = not todo.completed
    kv:put("todo:" .. id, json.encode(todo))
    return todo
end

-- -----------------------------------------------------------------------------
-- UPDATE TODO TEXT
-- -----------------------------------------------------------------------------
-- Updates the text content of an existing todo.
--
-- @param id (string) - The todo's ID
-- @param text (string) - The new text
-- @return (table|nil, string?) - Updated todo or (nil, error message)
-- -----------------------------------------------------------------------------
function M.update_todo(id, text)
    local todo = M.get_todo(id)
    if not todo then
        return nil, "Todo not found"
    end

    todo.text = text
    kv:put("todo:" .. id, json.encode(todo))
    return todo
end

-- -----------------------------------------------------------------------------
-- DELETE TODO
-- -----------------------------------------------------------------------------
-- Permanently removes a todo from storage.
--
-- @param id (string) - The todo's ID
-- @return (boolean, string?) - true on success, or (false, error message)
-- -----------------------------------------------------------------------------
function M.delete_todo(id)
    local todo = M.get_todo(id)
    if not todo then
        return false, "Todo not found"
    end

    kv:delete("todo:" .. id)
    return true
end

-- -----------------------------------------------------------------------------
-- CLEAR COMPLETED
-- -----------------------------------------------------------------------------
-- Deletes all todos that have been marked as completed.
-- Useful for batch cleanup after completing multiple tasks.
--
-- @return (number) - Count of deleted todos
-- -----------------------------------------------------------------------------
function M.clear_completed()
    local result = kv:list({ prefix = "todo:" })
    local deleted = 0

    for _, key in ipairs(result.keys) do
        local todo = kv:get(key.name, "json")
        if todo and todo.completed then
            kv:delete(key.name)
            deleted = deleted + 1
        end
    end

    return deleted
end

-- -----------------------------------------------------------------------------
-- TOGGLE ALL
-- -----------------------------------------------------------------------------
-- Sets all todos to the same completed state.
-- Used for "complete all" / "uncomplete all" functionality.
--
-- @param completed (boolean) - The state to set for all todos
-- @return (boolean) - Always returns true
-- -----------------------------------------------------------------------------
function M.toggle_all(completed)
    local result = kv:list({ prefix = "todo:" })

    for _, key in ipairs(result.keys) do
        local todo = kv:get(key.name, "json")
        if todo then
            todo.completed = completed
            kv:put(key.name, json.encode(todo))
        end
    end

    return true
end

-- -----------------------------------------------------------------------------
-- GET COUNTS
-- -----------------------------------------------------------------------------
-- Returns statistics about todos for display (e.g., "3 items left").
--
-- @return (table) - { total, active, completed } counts
-- -----------------------------------------------------------------------------
function M.get_counts()
    local result = kv:list({ prefix = "todo:" })
    local total = 0
    local active = 0
    local completed = 0

    for _, key in ipairs(result.keys) do
        local todo = kv:get(key.name, "json")
        if todo then
            total = total + 1
            if todo.completed then
                completed = completed + 1
            else
                active = active + 1
            end
        end
    end

    return {
        total = total,
        active = active,
        completed = completed
    }
end

return M
