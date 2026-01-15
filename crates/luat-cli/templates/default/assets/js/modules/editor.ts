/**
 * =============================================================================
 * TIPTAP RICH TEXT EDITOR MODULE (assets/js/modules/editor.ts)
 * =============================================================================
 * Provides rich text editing capabilities for blog posts using Tiptap.
 *
 * WHAT IS TIPTAP?
 *   Tiptap is a headless rich text editor framework built on ProseMirror.
 *   "Headless" means it provides the logic but you control the UI.
 *   This gives us full control over styling and toolbar design.
 *
 * HOW IT INTEGRATES WITH LUAT:
 *   1. Server renders a container with [data-editor] attribute
 *   2. This module finds those containers and initializes Tiptap
 *   3. A hidden <input> receives the HTML content for form submission
 *   4. When user submits form, the hidden input sends content to server
 *
 * EXTENSIONS USED:
 *   - StarterKit: Basic formatting (bold, italic, headings, lists, etc.)
 *   - Image: Allows inserting images via URL
 *   - Link: Converts URLs to clickable links
 *   - Placeholder: Shows placeholder text when empty
 *
 * DATA ATTRIBUTES (set on container element):
 *   data-editor           - Marks element as editor container
 *   data-editor-toolbar   - ID of toolbar element (optional)
 *   data-editor-input     - ID of hidden input for form submission
 *   data-editor-content   - Initial HTML content (alternative to innerHTML)
 *
 * TEMPLATE USAGE:
 *   See EditorContainer.luat and EditorToolbar.luat for server-side setup.
 * =============================================================================
 */

import { Editor } from '@tiptap/core';
import StarterKit from '@tiptap/starter-kit';
import Image from '@tiptap/extension-image';
import Link from '@tiptap/extension-link';
import Placeholder from '@tiptap/extension-placeholder';

// Type for tracking editor instances
interface EditorInstance {
  editor: Editor;
  destroy: () => void; 
}

// Map of element ID -> editor instance for lifecycle management
const editors: Map<string, EditorInstance> = new Map();

// -----------------------------------------------------------------------------
// CORE EDITOR INITIALIZATION
// -----------------------------------------------------------------------------

/**
 * Initialize a Tiptap editor on an element.
 *
 * @param elementId - ID of the container element
 * @param hiddenInputId - ID of hidden input to sync content to
 * @param initialContent - Initial HTML content (optional)
 * @returns The Editor instance, or null if elements not found
 *
 * CONTENT SYNC:
 *   The editor's onUpdate callback syncs HTML to the hidden input.
 *   This happens on every keystroke, ensuring form submission has latest content.
 */
export function initEditor(elementId: string, hiddenInputId: string, initialContent: string = ''): Editor | null {
  const element = document.getElementById(elementId);
  const hiddenInput = document.getElementById(hiddenInputId) as HTMLInputElement;

  if (!element || !hiddenInput) {
    console.warn(`Editor: Could not find element #${elementId} or input #${hiddenInputId}`);
    return null;
  }

  // Destroy existing editor if any (for HTMX swaps that re-render)
  const existing = editors.get(elementId);
  if (existing) {
    existing.destroy();
    editors.delete(elementId);
  }

  // Create Tiptap editor with extensions
  const editor = new Editor({
    element,
    extensions: [
      // StarterKit bundles common extensions
      StarterKit.configure({
        heading: {
          levels: [2, 3, 4]  // Only allow h2-h4 (h1 is for page title)
        }
      }),
      // Image extension for inserting images
      Image.configure({
        HTMLAttributes: {
          class: 'rounded-lg max-w-full'  // Tailwind styling
        }
      }),
      // Link extension for URLs
      Link.configure({
        openOnClick: false,  // Don't navigate when clicking in editor
        HTMLAttributes: {
          class: 'text-blue-600 underline'
        }
      }),
      // Placeholder shown when editor is empty
      Placeholder.configure({
        placeholder: 'Write your blog post content here...'
      })
    ],
    content: initialContent,
    editorProps: {
      attributes: {
        // Tailwind Typography "prose" classes for nice content styling
        class: 'prose prose-lg max-w-none focus:outline-none min-h-[200px] p-4'
      }
    },
    // Sync content to hidden input on every change
    onUpdate: ({ editor }) => {
      hiddenInput.value = editor.getHTML();
    }
  });

  // Set initial value to hidden input
  hiddenInput.value = editor.getHTML();

  // Track instance for cleanup
  editors.set(elementId, {
    editor,
    destroy: () => editor.destroy()
  });

  return editor;
}

// -----------------------------------------------------------------------------
// EDITOR WITH TOOLBAR
// -----------------------------------------------------------------------------

/**
 * Initialize editor with a connected toolbar.
 * The toolbar provides buttons for formatting actions.
 *
 * @param editorId - ID of editor container
 * @param toolbarId - ID of toolbar element
 * @param hiddenInputId - ID of hidden input
 * @param initialContent - Initial HTML content
 * @returns The Editor instance
 *
 * TOOLBAR ACTIONS:
 *   Buttons in the toolbar should have data-editor-action attribute.
 *   Supported actions: bold, italic, strike, h2, h3, bullet-list,
 *   ordered-list, blockquote, code, code-block, link, image, undo, redo
 */
export function initEditorWithToolbar(
  editorId: string,
  toolbarId: string,
  hiddenInputId: string,
  initialContent: string = ''
): Editor | null {
  const editor = initEditor(editorId, hiddenInputId, initialContent);
  if (!editor) return null;

  const toolbar = document.getElementById(toolbarId);
  if (!toolbar) return editor;

  // Setup toolbar buttons by finding all [data-editor-action] elements
  toolbar.querySelectorAll('[data-editor-action]').forEach((button) => {
    const action = button.getAttribute('data-editor-action');

    button.addEventListener('click', (e) => {
      e.preventDefault();

      // Execute the appropriate Tiptap command based on action
      switch (action) {
        case 'bold':
          editor.chain().focus().toggleBold().run();
          break;
        case 'italic':
          editor.chain().focus().toggleItalic().run();
          break;
        case 'strike':
          editor.chain().focus().toggleStrike().run();
          break;
        case 'h2':
          editor.chain().focus().toggleHeading({ level: 2 }).run();
          break;
        case 'h3':
          editor.chain().focus().toggleHeading({ level: 3 }).run();
          break;
        case 'bullet-list':
          editor.chain().focus().toggleBulletList().run();
          break;
        case 'ordered-list':
          editor.chain().focus().toggleOrderedList().run();
          break;
        case 'blockquote':
          editor.chain().focus().toggleBlockquote().run();
          break;
        case 'code':
          editor.chain().focus().toggleCode().run();
          break;
        case 'code-block':
          editor.chain().focus().toggleCodeBlock().run();
          break;
        case 'link':
          // Prompt for URL (could be replaced with custom modal)
          const url = prompt('Enter URL:');
          if (url) {
            editor.chain().focus().setLink({ href: url }).run();
          }
          break;
        case 'image':
          // Prompt for image URL (could be enhanced with upload)
          const imageUrl = prompt('Enter image URL:');
          if (imageUrl) {
            editor.chain().focus().setImage({ src: imageUrl }).run();
          }
          break;
        case 'undo':
          editor.chain().focus().undo().run();
          break;
        case 'redo':
          editor.chain().focus().redo().run();
          break;
      }

      // Update visual state of toolbar buttons
      updateToolbarState(toolbar, editor);
    });
  });

  // Update toolbar state when selection changes
  editor.on('selectionUpdate', () => {
    updateToolbarState(toolbar, editor);
  });

  // Set initial toolbar state
  updateToolbarState(toolbar, editor);

  return editor;
}

// -----------------------------------------------------------------------------
// TOOLBAR STATE MANAGEMENT
// -----------------------------------------------------------------------------

/**
 * Update toolbar button active states based on current selection.
 * Active buttons are highlighted to show what formatting is applied.
 */
function updateToolbarState(toolbar: HTMLElement, editor: Editor): void {
  toolbar.querySelectorAll('[data-editor-action]').forEach((button) => {
    const action = button.getAttribute('data-editor-action');
    let isActive = false;

    // Check if the formatting is active at current selection
    switch (action) {
      case 'bold':
        isActive = editor.isActive('bold');
        break;
      case 'italic':
        isActive = editor.isActive('italic');
        break;
      case 'strike':
        isActive = editor.isActive('strike');
        break;
      case 'h2':
        isActive = editor.isActive('heading', { level: 2 });
        break;
      case 'h3':
        isActive = editor.isActive('heading', { level: 3 });
        break;
      case 'bullet-list':
        isActive = editor.isActive('bulletList');
        break;
      case 'ordered-list':
        isActive = editor.isActive('orderedList');
        break;
      case 'blockquote':
        isActive = editor.isActive('blockquote');
        break;
      case 'code':
        isActive = editor.isActive('code');
        break;
      case 'code-block':
        isActive = editor.isActive('codeBlock');
        break;
      case 'link':
        isActive = editor.isActive('link');
        break;
    }

    // Toggle active styling classes
    button.classList.toggle('bg-gray-200', isActive);
    button.classList.toggle('text-blue-600', isActive);
  });
}

// -----------------------------------------------------------------------------
// EDITOR LIFECYCLE MANAGEMENT
// -----------------------------------------------------------------------------

/**
 * Get editor instance by element ID.
 * Useful for external code that needs to interact with the editor.
 */
export function getEditor(elementId: string): Editor | null {
  return editors.get(elementId)?.editor || null;
}

/**
 * Destroy editor instance and clean up resources.
 * Call this when the editor container is removed from DOM.
 */
export function destroyEditor(elementId: string): void {
  const instance = editors.get(elementId);
  if (instance) {
    instance.destroy();
    editors.delete(elementId);
  }
}

// -----------------------------------------------------------------------------
// AUTO-INITIALIZATION
// -----------------------------------------------------------------------------

/**
 * Auto-initialize editors on page load and after HTMX swaps.
 *
 * This function finds all elements with [data-editor] attribute
 * and initializes Tiptap on them. It's designed to work with
 * server-rendered content and HTMX partial updates.
 *
 * DATA ATTRIBUTE PATTERN:
 *   <div id="editor"
 *        data-editor
 *        data-editor-toolbar="toolbar-id"
 *        data-editor-input="hidden-input-id">
 *     Initial content goes here (innerHTML)
 *   </div>
 *
 * Called from app.ts on DOMContentLoaded and htmx:afterSwap events.
 */
export function initAutoEditors(): void {
  const initEditors = () => {
    document.querySelectorAll('[data-editor]').forEach((el) => {
      const editorId = el.id;
      const toolbarId = el.getAttribute('data-editor-toolbar') || '';
      const hiddenInputId = el.getAttribute('data-editor-input') || '';
      // Get initial content from innerHTML (server-rendered) or data attribute
      const initialContent = el.innerHTML.trim() || el.getAttribute('data-editor-content') || '';
      // Clear the container before initializing editor
      el.innerHTML = '';

      if (toolbarId) {
        initEditorWithToolbar(editorId, toolbarId, hiddenInputId, initialContent);
      } else {
        initEditor(editorId, hiddenInputId, initialContent);
      }
    });
  };

  // Init on page load
  initEditors();

  // Re-init after HTMX swaps (for dynamically loaded content)
  document.body.addEventListener('htmx:afterSwap', initEditors);
}
