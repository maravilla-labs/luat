-- Load a single blog post by slug (dynamic route parameter)
function load(ctx)
    local slug = ctx.params.slug

    -- In a real app, you'd fetch this from a database
    local posts = {
        ["hello-world"] = {
            title = "Hello World",
            date = "2024-01-01",
            content = "Welcome to my first blog post! This demonstrates how dynamic routes work in Luat. The [slug] directory captures the URL parameter, which is then available via ctx.params.slug in the load function."
        },
        ["getting-started"] = {
            title = "Getting Started with Luat",
            date = "2024-01-15",
            content = "Luat makes it easy to build server-rendered web applications. Start by creating a src/routes directory and adding +page.luat files for each route. Use +page.server.lua to load data server-side."
        },
        ["dynamic-routes"] = {
            title = "Dynamic Routes Explained",
            date = "2024-02-01",
            content = "Dynamic routes use [param] syntax to capture URL segments. For example, blog/[slug]/+page.luat matches /blog/hello-world and passes slug='hello-world' to the load function. You can also use [[optional]] for optional params and [...rest] for catch-all routes."
        }
    }

    local post = posts[slug]

    if not post then
        return {
            status = 404,
            title = "Not Found",
            post = {
                title = "Post Not Found",
                date = "",
                content = "The requested blog post could not be found."
            }
        }
    end

    return {
        title = post.title,
        post = post
    }
end
