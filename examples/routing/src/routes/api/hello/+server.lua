-- API endpoint: /api/hello
-- Returns JSON instead of HTML

function GET(ctx)
    return {
        status = 200,
        body = {
            message = "Hello from the API!",
            timestamp = os.time(),
            method = "GET"
        }
    }
end

function POST(ctx)
    local name = "World"
    if ctx.form and ctx.form.name then
        name = ctx.form.name
    end

    return {
        status = 200,
        body = {
            message = "Hello, " .. name .. "!",
            timestamp = os.time(),
            method = "POST",
            received = ctx.form
        }
    }
end
