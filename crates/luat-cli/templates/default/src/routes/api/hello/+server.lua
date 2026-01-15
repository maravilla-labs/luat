-- =============================================================================
-- HELLO API ENDPOINT (routes/api/hello/+server.lua)
-- =============================================================================
-- A simple REST API endpoint demonstrating the +server.lua pattern.
-- Returns JSON instead of HTML - perfect for building APIs.
--
-- ENDPOINTS:
--   GET  /api/hello - Returns a greeting with timestamp
--   POST /api/hello - Returns personalized greeting
--
-- +server.lua VS +page.server.lua:
--   +server.lua: Pure API endpoints, returns JSON, no template rendering
--   +page.server.lua: Page data, returns HTML via template
--
-- HTTP METHOD FUNCTIONS:
--   Export functions named after HTTP methods: GET, POST, PUT, DELETE, PATCH
--   Each receives ctx (context) with request info.
--
-- RESPONSE FORMAT:
--   Return a table with:
--   - status: HTTP status code (200, 201, 400, 404, etc.)
--   - body: Data to return (auto-converted to JSON)
--   - headers: Optional response headers
--
-- TESTING:
--   curl http://localhost:3000/api/hello
--   curl -X POST -d "name=World" http://localhost:3000/api/hello
-- =============================================================================

-- GET /api/hello
-- Returns a greeting message with current timestamp
function GET(ctx)
    return {
        status = 200,
        body = {
            message = "Hello from the API!",
            timestamp = os.time()  -- Unix timestamp
        }
    }
end

-- POST /api/hello
-- Returns personalized greeting using form data
-- Accepts: name (optional, defaults to "World")
function POST(ctx)
    -- Get name from form data, default to "World"
    local name = ctx.form and ctx.form.name or "World"

    return {
        status = 200,
        body = {
            message = "Hello, " .. name .. "!",
            received = ctx.form  -- Echo back received data
        }
    }
end
