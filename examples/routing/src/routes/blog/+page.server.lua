-- Load blog posts list
local utils = require("../../lib/utils")

function load(ctx)
    -- In a real app, you'd fetch this from a database
    local posts = {
        {
            slug = "hello-world",
            title = "Hello World",
            excerpt = "Welcome to my first blog post!"
        },
        {
            slug = "getting-started",
            title = "Getting Started with Luat",
            excerpt = "Learn how to build web apps with Luat routing."
        },
        {
            slug = "dynamic-routes",
            title = "Dynamic Routes Explained",
            excerpt = "Understanding [param] syntax in Luat routing."
        }
    }

    -- Use utils to format the current date
    local current_date = utils.format_date(os.time())

    return {
        title = "Blog",
        posts = posts,
        date = current_date
    }
end
