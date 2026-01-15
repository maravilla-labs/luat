/**
 * =============================================================================
 * DARK MODE MODULE (assets/js/modules/darkmode.ts)
 * =============================================================================
 * Handles dark mode toggle with localStorage persistence.
 *
 * HOW IT WORKS:
 *   1. initDarkMode() runs immediately on page load (before render)
 *   2. Reads preference from localStorage
 *   3. Applies "dark" class to <html> element
 *   4. Alpine.js component allows toggling via button click
 *
 * PREVENTING FLASH:  
 *   The initDarkMode function is called BEFORE Alpine starts.
 *   This ensures the correct theme is applied before the page renders,
 *   preventing a flash of the wrong theme.
 *
 * TAILWIND INTEGRATION:
 *   Tailwind's dark mode is configured with "class" strategy.
 *   When <html class="dark">, all dark:* utilities become active.
 *
 * USAGE:
 *   In app.ts: import { initDarkMode } from './modules/darkmode';
 *              initDarkMode();
 *
 *   In templates: x-data="darkMode" on a container element
 *                 @click="dark = !dark" on the toggle button
 * =============================================================================
 */

const STORAGE_KEY = 'darkMode';

/**
 * Initialize dark mode class on html element immediately.
 * Call this BEFORE Alpine starts to prevent flash of wrong theme.
 * Default is light mode unless explicitly set to dark in localStorage.
 */
export function initDarkMode(): void {
  const isDark = localStorage.getItem(STORAGE_KEY) === 'true';
  document.documentElement.classList.toggle('dark', isDark);
}

/**
 * Alpine.js data component for dark mode toggle.
 * Provides reactive state that syncs with localStorage.
 *
 * ALPINE COMPONENT PATTERN:
 *   - Returns an object with reactive properties
 *   - init() method runs when component initializes
 *   - $watch is an Alpine magic method for watching changes
 *
 * Note: This is also defined inline in app.ts for the alpine:init event.
 * Exported here for potential use in other contexts.
 */
export const darkModeComponent = () => ({
  dark: localStorage.getItem(STORAGE_KEY) === 'true',

  init() {
    // Watch for changes to 'dark' and sync to localStorage + DOM
    (this as any).$watch('dark', (val: boolean) => {
      localStorage.setItem(STORAGE_KEY, String(val));
      document.documentElement.classList.toggle('dark', val);
    });
  }
});
