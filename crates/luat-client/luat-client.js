/**
 * Luat Client Runtime — JavaScript DOM Layer
 *
 * Handles DOM operations (walking, patching, events) while WASM handles
 * computation (Lua VM, reactive store, expression evaluation).
 *
 * Two modes of operation:
 * 1. Full-bundle mode (new): Loads compiled Lua modules, re-renders templates on state change
 * 2. Legacy marker mode: Fine-grained DOM patching via base64-encoded markers
 *
 * Full-bundle mode is activated when a <script id="luat-bundle"> tag is present.
 *
 * Usage:
 *   <script type="module">
 *     import { initClient } from './luat-client.js';
 *     await initClient('/public/_luat');
 *   </script>
 */

let Module = null;
let initialized = false;

// ============================================================================
// Full-Bundle Mode State
// ============================================================================

// Template boundaries: moduleName → { startMarker, endMarker }
const templateBoundaries = new Map();

// Event handlers: element → { moduleName, handlerName, eventType }
const eventBindings = new Map();

// Discovered event types (for global delegation)
const discoveredEventTypes = new Set();

// ============================================================================
// Legacy Mode State (kept for backward compatibility)
// ============================================================================

// Subscriber registry: sub_id → { node, kind, attr? }
const subscribers = new Map();

// Event registry: event_type → Map<element, handlers[]>
const eventHandlers = new Map();

// Registered event types (to avoid duplicate listeners)
const registeredEventTypes = new Set();

// Pending update queue for RAF batching
let pendingUpdates = [];
let rafScheduled = false;

// ============================================================================
// Initialization
// ============================================================================

/**
 * Initialize the client runtime with a pre-loaded Emscripten Module.
 */
export async function initClientWithModule(createModule, options = {}) {
  if (initialized) return;

  Module = await createModule(options);

  const result = Module.ccall('luat_client_init', 'number', [], []);
  if (result !== 0) {
    throw new Error('Failed to initialize Luat client runtime');
  }

  initialized = true;

  // Check for full-bundle mode
  const bundleEl = document.getElementById('luat-bundle');
  if (bundleEl) {
    initFullBundleMode(bundleEl.textContent);
  } else {
    // Legacy marker mode
    scanAndBind();
  }
}

/**
 * Initialize the client runtime, loading WASM from the given path.
 */
export async function initClient(basePath = '.') {
  if (initialized) return;

  const base = basePath.endsWith('/') ? basePath : basePath + '/';

  const { default: createModule } = await import(`${base}luat-client-wasm.mjs`);

  return initClientWithModule(createModule, {
    locateFile: (path) => `${base}${path}`,
  });
}

// ============================================================================
// Full-Bundle Mode
// ============================================================================

/**
 * Initialize full-bundle mode: load bundle, scan for boundaries and events.
 */
function initFullBundleMode(bundleJson) {
  // Load the Lua bundle into WASM
  const result = callWasmVoid('luat_client_load_bundle', ['string'], [bundleJson]);

  // Scan DOM for template boundaries
  scanTemplateBoundaries();

  // Scan DOM for event markers
  scanEventMarkers();

  // Set up global event delegation
  setupGlobalDelegation();

  console.log(`[luat-client] Bundle mode: ${templateBoundaries.size} templates, ${discoveredEventTypes.size} event types`);
}

/**
 * Scan DOM for template boundary markers: <!--l:TB(moduleName)-->
 */
function scanTemplateBoundaries() {
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_COMMENT);

  while (walker.nextNode()) {
    const comment = walker.currentNode;
    const data = (comment.textContent || '').trim();

    if (data.startsWith('l:TB(') && data.endsWith(')')) {
      const moduleName = data.slice(5, -1); // Extract module name
      const endMarker = findEndMarker(comment);
      if (endMarker) {
        templateBoundaries.set(moduleName, {
          startMarker: comment,
          endMarker: endMarker,
        });
      }
    }
  }
}

/**
 * Scan DOM for event markers: <!--l:EV(handler_name)-->
 * Maps the next element sibling to the handler reference.
 */
function scanEventMarkers() {
  scanEventMarkersInRange(document.body);
}

/**
 * Scan a specific DOM range for event markers.
 */
function scanEventMarkersInRange(root) {
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_COMMENT);

  while (walker.nextNode()) {
    const comment = walker.currentNode;
    const data = (comment.textContent || '').trim();

    if (data.startsWith('l:EV(') && data.endsWith(')')) {
      const handlerName = data.slice(5, -1); // e.g., "click_0"

      // Find the next element sibling
      const element = findNextElementSibling(comment);
      if (!element) continue;

      // Parse handler name: "eventType_index"
      const underscoreIdx = handlerName.lastIndexOf('_');
      const eventType = handlerName.substring(0, underscoreIdx);

      // Find which template boundary this belongs to
      const moduleName = findModuleForNode(comment);
      if (!moduleName) continue;

      // Store the binding
      if (!eventBindings.has(element)) {
        eventBindings.set(element, []);
      }
      eventBindings.get(element).push({
        moduleName,
        handlerName,
        eventType,
      });

      discoveredEventTypes.add(eventType);
    }
  }
}

/**
 * Find which module a node belongs to by checking template boundaries.
 */
function findModuleForNode(node) {
  for (const [moduleName, { startMarker, endMarker }] of templateBoundaries) {
    if (isNodeBetween(node, startMarker, endMarker)) {
      return moduleName;
    }
  }
  return null;
}

/**
 * Check if a node is between two marker nodes in DOM order.
 */
function isNodeBetween(node, start, end) {
  const pos1 = start.compareDocumentPosition(node);
  const pos2 = end.compareDocumentPosition(node);
  // node is after start (FOLLOWING) and before end (PRECEDING)
  return (pos1 & Node.DOCUMENT_POSITION_FOLLOWING) !== 0 &&
         (pos2 & Node.DOCUMENT_POSITION_PRECEDING) !== 0;
}

/**
 * Set up one global event listener per discovered event type.
 */
function setupGlobalDelegation() {
  for (const eventType of discoveredEventTypes) {
    document.addEventListener(eventType, (event) => {
      // Walk up from event.target to find a registered element
      let current = event.target;
      while (current && current !== document) {
        const bindings = eventBindings.get(current);
        if (bindings) {
          for (const binding of bindings) {
            if (binding.eventType !== eventType) continue;

            // Call WASM: execute handler → get new HTML
            const newHtml = callWasm('luat_client_exec_named_handler',
              'number', ['string', 'string'],
              [binding.moduleName, binding.handlerName]
            );
            if (!newHtml) continue;

            // Replace DOM between template boundary markers
            replaceDomRange(binding.moduleName, newHtml);

            // Re-scan the new content for event markers
            rescanEventMarkers(binding.moduleName);
          }
          return; // Stop bubbling after handling
        }
        current = current.parentElement;
      }
    });
  }
}

/**
 * Replace the DOM content between template boundary markers.
 */
function replaceDomRange(moduleName, newHtml) {
  const boundary = templateBoundaries.get(moduleName);
  if (!boundary) return;

  const { startMarker, endMarker } = boundary;

  // Remove all nodes between start and end markers
  const range = document.createRange();
  range.setStartAfter(startMarker);
  range.setEndBefore(endMarker);
  range.deleteContents();

  // Insert new content
  const fragment = range.createContextualFragment(newHtml);
  startMarker.parentNode.insertBefore(fragment, endMarker);
}

/**
 * Re-scan event markers for a specific module after DOM replacement.
 * Clears old bindings for elements in this module's range.
 */
function rescanEventMarkers(moduleName) {
  const boundary = templateBoundaries.get(moduleName);
  if (!boundary) return;

  // Clear old bindings for elements in this module's range
  for (const [element, bindings] of eventBindings) {
    const filtered = bindings.filter(b => b.moduleName !== moduleName);
    if (filtered.length === 0) {
      eventBindings.delete(element);
    } else {
      eventBindings.set(element, filtered);
    }
  }

  // Re-scan the range between the boundary markers
  const { startMarker, endMarker } = boundary;
  let current = startMarker.nextSibling;
  const tempContainer = document.createDocumentFragment();

  // Collect nodes in range for scanning
  while (current && current !== endMarker) {
    // We need to scan comments in this range
    if (current.nodeType === Node.COMMENT_NODE) {
      const data = (current.textContent || '').trim();
      if (data.startsWith('l:EV(') && data.endsWith(')')) {
        const handlerName = data.slice(5, -1);
        const element = findNextElementSibling(current);
        if (element) {
          const underscoreIdx = handlerName.lastIndexOf('_');
          const eventType = handlerName.substring(0, underscoreIdx);

          if (!eventBindings.has(element)) {
            eventBindings.set(element, []);
          }
          eventBindings.get(element).push({
            moduleName,
            handlerName,
            eventType,
          });

          discoveredEventTypes.add(eventType);
        }
      }
    }
    current = current.nextSibling;
  }
}

// ============================================================================
// Legacy Marker Mode (backward compatibility)
// ============================================================================

const MARKER_PREFIX = 'l:';
const END_MARKER_PREFIX = '/l';
const SHOW_COMMENT = 128;

/**
 * Scan the DOM for markers and set up reactive bindings (legacy mode).
 */
function scanAndBind() {
  const body = document.body;
  if (!body) return;

  const walker = document.createTreeWalker(body, SHOW_COMMENT);
  let nextSubId = 0;

  while (walker.nextNode()) {
    const comment = walker.currentNode;
    const data = comment.textContent || '';

    if (!data.startsWith(MARKER_PREFIX)) continue;

    const payload = data.slice(MARKER_PREFIX.length);

    // Check for new-style text markers (TB, EV)
    if (payload.startsWith('TB(') || payload.startsWith('EV(')) continue;

    const markerJson = callWasm('luat_client_decode_marker', 'number', ['string'], [payload]);
    if (!markerJson) continue;

    const marker = JSON.parse(markerJson);

    switch (marker.type) {
      case 'expr': {
        const textNode = findBoundedTextNode(comment);
        if (!textNode) break;

        const subId = nextSubId++;
        for (const dep of marker.deps) {
          callWasmVoid('luat_client_register_state', ['string', 'string'], [dep, JSON.stringify(marker.value)]);
        }
        callWasmVoid('luat_client_register_subscriber',
          ['number', 'string', 'string', 'string', 'string'],
          [subId, 'text', marker.expr, JSON.stringify(marker.deps), null]
        );
        subscribers.set(subId, { node: textNode, kind: 'text' });
        break;
      }

      case 'event': {
        const element = findNextElementSibling(comment);
        if (!element) break;
        for (const binding of marker.bindings) {
          registerLegacyEvent(element, binding.event, binding.handler, binding.modifiers);
        }
        break;
      }

      case 'template_boundary': break;
      case 'component': break;

      case 'attr': {
        const element = findNextElementSibling(comment);
        if (!element) break;
        for (const binding of marker.bindings) {
          const subId = nextSubId++;
          for (const dep of binding.deps) {
            callWasmVoid('luat_client_register_state', ['string', 'string'], [dep, JSON.stringify(binding.value)]);
          }
          callWasmVoid('luat_client_register_subscriber',
            ['number', 'string', 'string', 'string', 'string'],
            [subId, 'attr', binding.expr, JSON.stringify(binding.deps), binding.attr]
          );
          subscribers.set(subId, { node: element, kind: 'attr', attr: binding.attr });
        }
        break;
      }

      case 'derived_def': {
        callWasmVoid('luat_client_register_derived',
          ['string', 'string', 'string'],
          [marker.name, marker.expr, JSON.stringify(marker.deps)]
        );
        break;
      }
    }
  }

  console.log(`[luat-client] Legacy mode: ${subscribers.size} subscribers, ${registeredEventTypes.size} event types`);
}

// ============================================================================
// Legacy Event Delegation
// ============================================================================

function registerLegacyEvent(element, eventType, handler, modifiers) {
  if (!eventHandlers.has(eventType)) {
    eventHandlers.set(eventType, new Map());
  }

  const typeMap = eventHandlers.get(eventType);
  if (!typeMap.has(element)) {
    typeMap.set(element, []);
  }
  typeMap.get(element).push({ handler, modifiers });

  if (!registeredEventTypes.has(eventType)) {
    registeredEventTypes.add(eventType);

    document.addEventListener(eventType, (event) => {
      const typeHandlers = eventHandlers.get(eventType);
      if (!typeHandlers) return;

      let current = event.target;
      while (current && current !== document) {
        if (typeHandlers.has(current)) {
          const handlers = typeHandlers.get(current);
          for (const { handler, modifiers } of handlers) {
            if (modifiers.includes('preventDefault')) event.preventDefault();
            if (modifiers.includes('stopPropagation')) event.stopPropagation();
            if (modifiers.includes('self') && event.target !== current) continue;

            const updatesJson = callWasm('luat_client_exec_handler', 'number', ['string'], [handler]);
            if (updatesJson) {
              const updates = JSON.parse(updatesJson);
              if (updates.length > 0) {
                queueUpdates(updates);
              }
            }
          }
          return;
        }
        current = current.parentElement;
      }
    });
  }
}

// ============================================================================
// Legacy DOM Patching
// ============================================================================

function queueUpdates(updates) {
  pendingUpdates.push(...updates);
  if (!rafScheduled) {
    rafScheduled = true;
    requestAnimationFrame(flushUpdates);
  }
}

function flushUpdates() {
  rafScheduled = false;
  const updates = pendingUpdates;
  pendingUpdates = [];

  for (const update of updates) {
    const sub = subscribers.get(update.sub_id);
    if (!sub) continue;

    switch (sub.kind) {
      case 'text':
        sub.node.textContent = update.value;
        break;
      case 'attr':
        sub.node.setAttribute(update.attr || sub.attr, update.value);
        break;
      case 'component': {
        const range = document.createRange();
        range.setStartAfter(sub.node);
        range.setEndBefore(sub.endMarker);
        range.deleteContents();
        const fragment = range.createContextualFragment(update.value);
        sub.node.parentNode.insertBefore(fragment, sub.endMarker);
        break;
      }
    }
  }
}

// ============================================================================
// DOM Helpers
// ============================================================================

function findBoundedTextNode(startMarker) {
  let current = startMarker.nextSibling;
  while (current) {
    if (current.nodeType === Node.TEXT_NODE) return current;
    if (current.nodeType === Node.COMMENT_NODE) return null;
    if (current.nodeType === Node.ELEMENT_NODE) return null;
    current = current.nextSibling;
  }
  return null;
}

function findEndMarker(startMarker) {
  let current = startMarker.nextSibling;
  let depth = 0;
  while (current) {
    if (current.nodeType === Node.COMMENT_NODE) {
      const data = (current.textContent || '').trim();
      if (data.startsWith('l:TB(')) {
        depth++;
      } else if (data === '/l') {
        if (depth === 0) return current;
        depth--;
      }
    }
    current = current.nextSibling;
  }
  return null;
}

function findNextElementSibling(node) {
  let current = node.nextSibling;
  while (current) {
    if (current.nodeType === Node.ELEMENT_NODE) return current;
    current = current.nextSibling;
  }
  return null;
}

// ============================================================================
// WASM Bridge Helpers
// ============================================================================

function callWasm(name, returnType, argTypes, args) {
  const ptr = Module.ccall(name, returnType, argTypes, args);
  if (ptr === 0) return null;
  const result = Module.UTF8ToString(ptr);
  Module._luat_client_free_string(ptr);
  return result;
}

function callWasmVoid(name, argTypes, args) {
  Module.ccall(name, 'number', argTypes, args);
}

// ============================================================================
// Public API
// ============================================================================

export function setState(name, value) {
  if (!initialized) throw new Error('Luat client not initialized');
  const updatesJson = callWasm('luat_client_set_state', 'number', ['string', 'string'], [name, JSON.stringify(value)]);
  if (updatesJson) {
    const updates = JSON.parse(updatesJson);
    if (updates.length > 0) {
      queueUpdates(updates);
    }
  }
}

export function evalExpr(expr) {
  if (!initialized) throw new Error('Luat client not initialized');
  return callWasm('luat_client_eval_expr', 'number', ['string'], [expr]);
}

export function isInitialized() {
  return initialized;
}

export default {
  initClient,
  initClientWithModule,
  setState,
  evalExpr,
  isInitialized,
};
