-- =============================================================================
-- BLOG EDIT PAGE SERVER (routes/blog/[slug]/edit/+page.server.lua)
-- =============================================================================
-- Server-side logic for the blog post edit page.
-- Handles loading post data and processing edit/delete actions.
--
-- URL: /blog/:slug/edit
--
-- LOAD FUNCTION:
--   Returns existing post data for the edit form.
--   Returns 404 if post not found.
--
-- ACTIONS:
--   edit: Updates post with new title, excerpt, content, image_url
--   delete: Permanently removes the post
--
-- ERROR HANDLING:
--   Uses fail() to return form data back to page on validation errors.
--   This allows the form to re-populate with user's input.
-- =============================================================================

local blog = require("lib/blog")

-- -----------------------------------------------------------------------------
-- LOAD FUNCTION
-- -----------------------------------------------------------------------------
-- Fetch the post data for editing.
-- The slug comes from the URL parameter ctx.params.slug.
function load(ctx)
    local slug = ctx.params.slug
    local post = blog.get_post(slug)

    -- Return 404 if post doesn't exist
    if not post then
        return {
            status = 404,
            title = "Not Found",
            error = "Post not found"
        }
    end

    return {
        title = "Edit: " .. post.title,
        post = post
    }
end

-- -----------------------------------------------------------------------------
-- ACTIONS TABLE
-- -----------------------------------------------------------------------------
-- Named form handlers called via ?/actionName in URL.
actions = {
    -- -------------------------------------------------------------------------
    -- EDIT ACTION
    -- -------------------------------------------------------------------------
    -- Updates post fields. Title is required.
    -- On success, can redirect to post view (currently commented out).
    -- On failure, returns form data to repopulate inputs.
    edit = function(ctx)
        local slug = ctx.params.slug
        local form = ctx.form or {}

        -- Validate required fields
        if not form.title or form.title == "" then
            return fail(400, {
                error = "Title is required",
                title = form.title,
                excerpt = form.excerpt,
                content = form.content
            })
        end

        -- Attempt to update the post
        local post, err = blog.update_post(slug, {
            title = form.title,
            excerpt = form.excerpt,
            content = form.content,
            image_url = form.image_url
        })

        -- Handle update failure
        if not post then
            return fail(500, {
                error = err,
                title = form.title,
                excerpt = form.excerpt, 
                content = form.content
            })
        end

        -- Success response
        -- HX-Redirect header would redirect HTMX clients
        return {
            success = true,
            headers = {
            ["HX-Redirect"] = "/blog/" .. slug
            }
        }
    end,

    -- -------------------------------------------------------------------------
    -- DELETE ACTION
    -- -------------------------------------------------------------------------
    -- Permanently removes the post.
    -- Redirects to blog list on success.
    delete = function(ctx)
        local slug = ctx.params.slug
        local success, err = blog.delete_post(slug)

        if not success then
            return fail(404, { error = err })
        end

        -- Redirect to blog list after deletion
        return {
            deleted = true,
            headers = {
                ["HX-Redirect"] = "/blog"
            }
        }
    end
}
