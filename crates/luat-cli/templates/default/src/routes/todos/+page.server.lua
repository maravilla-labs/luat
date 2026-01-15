-- =============================================================================
-- TODO APPLICATION SERVER (routes/todos/+page.server.lua)
-- =============================================================================
-- Server-side logic for the todo application.
-- Demonstrates multiple actions for different HTMX interactions.
--
-- REQUIRE ALIAS:
--   require("$lib/todos") - The $lib alias points to src/lib/
--   This is cleaner than relative paths like "../../lib/todos"
--
-- ACTIONS OVERVIEW:
--   Actions handle form submissions (POST) and can be called via:
--   - hx-post="?/actionName" - POST with action
--   - hx-get="?/actionName" - GET with action (for filters)
--
-- FRAGMENT RESPONSES:
--   Each action returns data that gets rendered by the corresponding
--   fragment template in (fragments)/ directory:
--   - ?/add -> (fragments)/add.luat
--   - ?/toggle -> (fragments)/toggle.luat
--   - etc.
--
-- OUT-OF-BAND UPDATES:
--   The hx-swap-oob="true" attribute in fragments allows updating
--   multiple page elements from a single response (e.g., updating
--   both a todo item and the footer count simultaneously).
-- =============================================================================

-- Import the todos module (using $lib alias)
local todos = require("$lib/todos")

-- =============================================================================
-- LOAD FUNCTION
-- =============================================================================
-- Runs on initial page load (GET /todos)
-- Returns data for the full page render
function load(ctx)
    -- Get filter from query string (default: "all")
    local filter = ctx.query.filter or "all"
    return {
        title = "Todos",
        todos = todos.get_todos(filter),  -- Filtered todo list
        counts = todos.get_counts(),       -- {total, active, completed}
        filter = filter                    -- Current filter for UI state
    }
end

-- =============================================================================
-- ACTIONS TABLE
-- =============================================================================
-- Each action is a function that handles a specific user interaction.
-- Actions receive ctx with form data, query params, etc.
actions = {
    -- -------------------------------------------------------------------------
    -- FILTER ACTIONS
    -- -------------------------------------------------------------------------
    -- These handle clicking the All/Active/Completed filter buttons.
    -- They return the full todo list for the selected filter.

    all = function(ctx)
        return {
            todos = todos.get_todos("all"),
            counts = todos.get_counts(),
            filter = "all"
        }
    end,

    active = function(ctx)
        return {
            todos = todos.get_todos("active"),
            counts = todos.get_counts(),
            filter = "active"
        }
    end,

    completed = function(ctx)
        return {
            todos = todos.get_todos("completed"),
            counts = todos.get_counts(),
            filter = "completed"
        }
    end,

    -- -------------------------------------------------------------------------
    -- ADD ACTION
    -- -------------------------------------------------------------------------
    -- Creates a new todo from the input form.
    -- Validates that text is not empty.
    add = function(ctx)
        -- Trim whitespace from input
        local text = (ctx.form.text or ""):match("^%s*(.-)%s*$")
        if text == "" then
            return fail(400, { error = "Text is required" })
        end

        -- Create the todo and return it for rendering
        local todo = todos.add_todo(text)
        return {
            todo = todo,
            counts = todos.get_counts()  -- For out-of-band count update
        }
    end,

    -- -------------------------------------------------------------------------
    -- TOGGLE ACTION
    -- -------------------------------------------------------------------------
    -- Toggles the completed state of a todo.
    toggle = function(ctx)
        local id = ctx.form.id
        if not id then
            return fail(400, { error = "ID is required" })
        end

        local todo, err = todos.toggle_todo(id)
        if not todo then
            return fail(404, { error = err })
        end

        return {
            todo = todo,
            counts = todos.get_counts()
        }
    end,

    -- -------------------------------------------------------------------------
    -- EDIT FORM ACTION
    -- -------------------------------------------------------------------------
    -- Returns the edit form for a todo (triggered by double-click or Enter).
    editForm = function(ctx)
        -- ID can come from query (GET) or form (POST)
        local id = ctx.query.id or ctx.form.id
        if not id then
            return fail(400, { error = "ID is required" })
        end

        local todo = todos.get_todo(id)
        if not todo then
            return fail(404, { error = "Todo not found" })
        end

        -- editing=true tells the fragment to render the edit form
        return { todo = todo, editing = true }
    end,

    -- -------------------------------------------------------------------------
    -- EDIT ACTION
    -- -------------------------------------------------------------------------
    -- Saves the edited todo text.
    edit = function(ctx)
        local id = ctx.form.id
        local text = (ctx.form.text or ""):match("^%s*(.-)%s*$")

        if not id then
            return fail(400, { error = "ID is required" })
        end
        if text == "" then
            -- If empty, cancel edit and return original todo
            local todo = todos.get_todo(id)
            return { todo = todo }
        end

        local todo, err = todos.update_todo(id, text)
        if not todo then
            return fail(404, { error = err })
        end

        return { todo = todo }
    end,

    -- -------------------------------------------------------------------------
    -- CANCEL EDIT ACTION
    -- -------------------------------------------------------------------------
    -- Cancels editing and returns the original todo (triggered by Escape).
    cancelEdit = function(ctx)
        local id = ctx.form.id
        if not id then
            return fail(400, { error = "ID is required" })
        end

        local todo = todos.get_todo(id)
        if not todo then
            return fail(404, { error = "Todo not found" })
        end

        return { todo = todo }
    end,

    -- -------------------------------------------------------------------------
    -- DELETE ACTION
    -- -------------------------------------------------------------------------
    -- Removes a todo permanently.
    delete = function(ctx)
        local id = ctx.form.id
        if not id then
            return fail(400, { error = "ID is required" })
        end

        local success, err = todos.delete_todo(id)
        if not success then
            return fail(404, { error = err })
        end

        return {
            deleted = true,
            id = id,
            counts = todos.get_counts()
        }
    end,

    -- -------------------------------------------------------------------------
    -- CLEAR ACTION
    -- -------------------------------------------------------------------------
    -- Deletes all completed todos.
    clear = function(ctx)
        local filter = ctx.query.filter or "all"
        todos.clear_completed()
        return {
            todos = todos.get_todos(filter),
            counts = todos.get_counts(),
            filter = filter
        }
    end,

    -- -------------------------------------------------------------------------
    -- TOGGLE ALL ACTION
    -- -------------------------------------------------------------------------
    -- Sets all todos to the same completed state.
    toggleAll = function(ctx)
        local filter = ctx.query.filter or "all"
        -- Convert string "true"/"false" to boolean
        local completed = ctx.form.completed == "true"
        todos.toggle_all(completed)
        return {
            todos = todos.get_todos(filter),
            counts = todos.get_counts(),
            filter = filter
        }
    end
}
