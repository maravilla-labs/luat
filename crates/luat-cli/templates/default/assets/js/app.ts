/**
 * =============================================================================
 * MAIN APPLICATION ENTRY POINT (assets/js/app.ts)
 * =============================================================================
 * This is the main JavaScript/TypeScript entry point for the Luat application.
 * It initializes all client-side libraries and sets up their interactions.
 *
 * LIBRARIES USED:
 *   - htmx: Enables HTML-driven AJAX requests via declarative attributes
 *   - idiomorph: Smart DOM diffing for htmx swaps (preserves focus, animations)
 *   - Alpine.js: Lightweight reactivity for client-side state
 *   - View Transitions API: Native browser animations between page states
 *
 * WHY THESE LIBRARIES?
 *   Luat's philosophy is "HTML-first" - most logic lives on the server.
 *   These libraries enhance the HTML without requiring a full SPA framework.
 *   The result is fast, accessible, and progressively enhanced.
 *
 * BUILD PROCESS:
 *   This file is bundled by Vite (configured in luat.toml).
 *   The output goes to static/js/app.js and is loaded in app.html.
 * =============================================================================
 */

import htmx from 'htmx.org';
// @ts-ignore - idiomorph doesn't have types
import { Idiomorph } from 'idiomorph/dist/idiomorph.esm.js';
// htmx extension: updates document.title from HX-Title response header
import { title } from '@maravilla-labs/htmx-ext-title';
import Alpine from 'alpinejs';
import { initDarkMode } from './modules/darkmode';
import { initAutoEditors } from './modules/editor';
import { initConfirmDialog } from './modules/confirm';

// -----------------------------------------------------------------------------
// DARK MODE INITIALIZATION
// -----------------------------------------------------------------------------
// Initialize dark mode BEFORE Alpine starts to prevent flash of wrong theme.
// This reads from localStorage and applies the class immediately.
initDarkMode();

// Register htmx title extension (enables HX-Title response header)
title(htmx);

// -----------------------------------------------------------------------------
// GLOBAL EXPORTS
// -----------------------------------------------------------------------------
// Make htmx and Alpine available globally for:
// - Inline scripts in templates
// - Browser DevTools debugging
// - Third-party integrations
(window as any).htmx = htmx;
(window as any).Alpine = Alpine;

// -----------------------------------------------------------------------------
// VIEW TRANSITIONS CONFIGURATION
// -----------------------------------------------------------------------------
// Enable View Transitions for all HTMX swaps.
// This creates smooth animations when content changes.
// Works automatically with view-transition-name CSS property.
htmx.config.globalViewTransitions = true;

// -----------------------------------------------------------------------------
// IDIOMORPH EXTENSION
// -----------------------------------------------------------------------------
// Register a custom htmx swap strategy called "morph".
// Unlike innerHTML swap, morphing intelligently diffs the DOM.
//
// BENEFITS:
//   - Preserves focus state in form inputs
//   - Maintains scroll position
//   - Enables FLIP animations (elements animate to new positions)
//   - Keeps Alpine.js state intact
//
// USAGE in templates:
//   hx-swap="morph"
htmx.defineExtension('morph', {
  isInlineSwap: function(swapStyle: string) {
    return swapStyle === 'morph';
  },
  handleSwap: function(swapStyle: string, target: Node, fragment: Node) {
    if (swapStyle === 'morph') {
      // Assign view-transition-name to each todo item before morph
      // This enables FLIP animations (First, Last, Invert, Play)
      assignViewTransitionNames(target as Element);

      // Use View Transitions API if available for FLIP effect
      if ('startViewTransition' in document) {
        (document as any).startViewTransition(() => {
          Idiomorph.morph(target, fragment, {
            morphStyle: 'outerHTML',
            callbacks: {
              beforeNodeAdded: (node: Node) => {
                // Assign transition name to new items for entry animation
                if ((node as Element).id?.startsWith('todo-')) {
                  (node as HTMLElement).style.viewTransitionName = (node as Element).id;
                }
                return true;
              }
            }
          });
        });
        return [target];
      } else {
        // Fallback for browsers without View Transitions
        Idiomorph.morph(target, fragment, { morphStyle: 'outerHTML' });
        return [target];
      }
    }
    return false;
  }
});

// -----------------------------------------------------------------------------
// VIEW TRANSITION HELPERS
// -----------------------------------------------------------------------------
// Assign unique view-transition-name to each todo item.
// This enables browser to animate items smoothly when they reorder.
function assignViewTransitionNames(container: Element): void {
  const items = container.querySelectorAll('li[id^="todo-"]');
  items.forEach((item) => {
    (item as HTMLElement).style.viewTransitionName = item.id;
  });
}

// Initialize View Transition names on page load
function initViewTransitionNames(): void {
  const todoList = document.getElementById('todo-list');
  if (todoList) {
    assignViewTransitionNames(todoList);

    // Re-assign after any HTMX swap to keep transitions working
    document.body.addEventListener('htmx:afterSwap', (event: Event) => {
      const detail = (event as CustomEvent).detail;
      const target = detail?.target as HTMLElement;
      if (target?.id === 'todo-list' || target?.closest('#todo-list')) {
        assignViewTransitionNames(document.getElementById('todo-list')!);
      }
    });
  }
}

// -----------------------------------------------------------------------------
// ALPINE.JS COMPONENTS
// -----------------------------------------------------------------------------
// Register Alpine components via alpine:init event.
// This ensures components are registered BEFORE Alpine processes the DOM.
//
// DARK MODE COMPONENT:
//   Used in the layout for the theme toggle button.
//   Syncs state to localStorage and updates document class.
document.addEventListener('alpine:init', () => {
  Alpine.data('darkMode', () => ({
    dark: localStorage.getItem('darkMode') === 'true',
    init() {
      // $watch is an Alpine magic method that watches for changes
      (this as any).$watch('dark', (val: boolean) => {
        localStorage.setItem('darkMode', String(val));
        document.documentElement.classList.toggle('dark', val);
      });
    }
  }));
  console.log('Alpine darkMode component registered');
});

// Start Alpine.js after components are registered
Alpine.start();

// -----------------------------------------------------------------------------
// DOM READY INITIALIZATION
// -----------------------------------------------------------------------------
// Final setup after DOM is fully loaded.
document.addEventListener('DOMContentLoaded', () => {
  console.log('Luat app loaded with htmx + idiomorph + Alpine.js + View Transitions');

  // Setup View Transition names for FLIP animations
  initViewTransitionNames();

  // Initialize Tiptap rich text editors on elements with [data-editor]
  initAutoEditors();

  // Initialize custom confirm dialog for htmx hx-confirm attributes
  initConfirmDialog();
});
