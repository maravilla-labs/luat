/**
 * =============================================================================
 * TYPE DECLARATIONS (assets/js/types/htmx.d.ts)
 * =============================================================================
 * TypeScript type declarations for libraries that don't ship their own types.
 *
 * WHY TYPE DECLARATIONS?
 *   TypeScript needs type information to provide IntelliSense, error checking,
 *   and autocompletion. Some libraries (especially older ones) don't include
 *   TypeScript types, so we declare them ourselves.
 *
 * WHAT'S DECLARED HERE:
 *   1. htmx.org - The htmx library for HTML-driven AJAX
 *   2. idiomorph - Smart DOM morphing library
 *
 * HOW TO ADD MORE:
 *   If you import a library without types, you can either:
 *   - Install @types/library-name from npm (if available)
 *   - Add a declaration here using 'declare module'
 *   - Use // @ts-ignore as a last resort
 *
 * TYPESCRIPT CONFIG:
 *   tsconfig.json includes this types directory in the compilation.
 * =============================================================================
 */

/**
 * htmx.org module declaration
 * Provides types for htmx configuration, extensions, and API methods.
 */
declare module 'htmx.org' {
  /**
   * htmx global configuration options.
   * These can be set via htmx.config.* or data attributes on body.
   */
  interface HtmxConfig {
    /** Default swap style (innerHTML, outerHTML, etc.) */
    defaultSwapStyle: string;
    /** Enable browser history integration */
    historyEnabled: boolean;
    /** Number of pages to cache in history */
    historyCacheSize: number;
    /** Refresh page on history cache miss */
    refreshOnHistoryMiss: boolean;
    /** Delay before settling (ms) */
    defaultSettleDelay: number;
    /** Delay before swap (ms) */
    defaultSwapDelay: number;
    /** Include default htmx indicator styles */
    includeIndicatorStyles: boolean;
    /** Class added during request */
    indicatorClass: string;
    /** Class added to element making request */
    requestClass: string;
    /** Class added to new elements */
    addedClass: string;
    /** Class during settling phase */
    settlingClass: string;
    /** Class during swap phase */
    swappingClass: string;
    /** Allow eval() in response */
    allowEval: boolean;
    /** Allow script tags in response */
    allowScriptTags: boolean;
    /** Nonce for inline scripts */
    inlineScriptNonce: string;
    /** Attributes to preserve during settle */
    attributesToSettle: string[];
    /** Send cookies with requests */
    withCredentials: boolean;
    /** Request timeout (ms) */
    timeout: number;
    /** WebSocket reconnect delay */
    wsReconnectDelay: string;
    /** WebSocket binary type */
    wsBinaryType: string;
    /** Selector for elements to disable htmx on */
    disableSelector: string;
    /** Scroll behavior for boosted links */
    scrollBehavior: string;
    /** Focus after swap */
    defaultFocusScroll: boolean;
    /** Add cache buster to GET requests */
    getCacheBusterParam: boolean;
    /** Enable View Transitions API for swaps */
    globalViewTransitions: boolean;
    /** HTTP methods that use URL params */
    methodsThatUseUrlParams: string[];
    /** Only allow same-origin requests */
    selfRequestsOnly: boolean;
    /** Ignore HX-Title header */
    ignoreTitle: boolean;
    /** Scroll into view on boost */
    scrollIntoViewOnBoost: boolean;
    /** Internal: cached trigger specs */
    triggerSpecsCache: null | object;
  }

  /**
   * htmx extension interface.
   * Extensions can customize swap behavior, transform responses, etc.
   */
  interface HtmxExtension {
    /** Return true if this extension handles the swap style */
    isInlineSwap?: (swapStyle: string) => boolean;
    /** Custom swap implementation */
    handleSwap?: (swapStyle: string, target: Node, fragment: Node, settleInfo?: unknown) => boolean | Node[];
    /** Hook into htmx events */
    onEvent?: (name: string, evt: Event) => boolean | void;
    /** Transform response before processing */
    transformResponse?: (text: string, xhr: XMLHttpRequest, elt: Element) => string;
    /** Initialize the extension */
    init?: (api: unknown) => void;
    /** Return selectors for elements to process */
    getSelectors?: () => string[];
  }

  /**
   * Main htmx API interface.
   * Provides methods for AJAX, DOM manipulation, and event handling.
   */
  interface Htmx {
    /** Global configuration */
    config: HtmxConfig;
    /** Make an AJAX request */
    ajax: (verb: string, url: string, options?: Record<string, unknown>) => void;
    /** Register a custom extension */
    defineExtension: (name: string, extension: HtmxExtension) => void;
    /** Process element for htmx attributes */
    process: (elt: Element) => void;
    /** Add event listener */
    on: (event: string, handler: (evt: Event) => void) => void;
    /** Remove event listener */
    off: (event: string, handler: (evt: Event) => void) => void;
    /** Trigger a custom event */
    trigger: (elt: Element, event: string, detail?: unknown) => void;
    /** Find element by selector */
    find: (selector: string) => Element | null;
    /** Find all elements by selector */
    findAll: (selector: string) => Element[];
    /** Find closest ancestor matching selector */
    closest: (elt: Element, selector: string) => Element | null;
    /** Remove element with optional delay */
    remove: (elt: Element, delay?: number) => void;
    /** Add class with optional delay */
    addClass: (elt: Element, className: string, delay?: number) => void;
    /** Remove class with optional delay */
    removeClass: (elt: Element, className: string, delay?: number) => void;
    /** Toggle class */
    toggleClass: (elt: Element, className: string) => void;
    /** Remove class from siblings, add to element */
    takeClass: (elt: Element, className: string) => void;
    /** Swap content into target */
    swap: (target: Element, content: string, swapSpec?: object) => void;
    /** htmx version string */
    version: string;
  }

  const htmx: Htmx;
  export default htmx;
}

/**
 * Idiomorph module declaration
 * Smart DOM morphing that preserves focus, animations, and state.
 */
declare module 'idiomorph/dist/idiomorph.esm.js' {
  /**
   * Options for Idiomorph.morph()
   */
  interface MorphOptions {
    /** How to apply the morph: innerHTML or outerHTML */
    morphStyle?: 'innerHTML' | 'outerHTML';
    /** Skip morphing the active element */
    ignoreActive?: boolean;
    /** Preserve active element's value */
    ignoreActiveValue?: boolean;
    /** Head element handling options */
    head?: {
      style?: 'merge' | 'append' | 'morph' | 'none';
    };
    /** Lifecycle callbacks for fine-grained control */
    callbacks?: {
      /** Called before adding a node, return false to skip */
      beforeNodeAdded?: (node: Node) => boolean;
      /** Called after a node is added */
      afterNodeAdded?: (node: Node) => void;
      /** Called before morphing a node, return false to skip */
      beforeNodeMorphed?: (oldNode: Node, newNode: Node) => boolean;
      /** Called after a node is morphed */
      afterNodeMorphed?: (oldNode: Node, newNode: Node) => void;
      /** Called before removing a node, return false to keep */
      beforeNodeRemoved?: (node: Node) => boolean;
      /** Called after a node is removed */
      afterNodeRemoved?: (node: Node) => void;
    };
  }

  /**
   * Idiomorph API
   * morph() intelligently updates the DOM while preserving state.
   */
  export const Idiomorph: {
    morph: (oldNode: Node, newContent: Node | string | NodeList | HTMLCollection, options?: MorphOptions) => void;
  };
}
