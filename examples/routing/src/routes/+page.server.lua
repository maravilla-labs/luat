-- Server-side load function for the home page
-- This runs on every request and returns data for the template

function load(ctx)
    return {
        title = "Home",
        message = "Hello from the server! This message was loaded via +page.server.lua"
    }
end
