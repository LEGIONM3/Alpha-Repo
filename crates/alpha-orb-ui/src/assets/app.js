// ── Alpha Chat — Streaming IPC Frontend Logic ──
//
// Communicates with Rust via wry IPC:
//   JS → Rust:  window.ipc.postMessage(JSON)
//   Rust → JS:  window.__alpha_receive(object)
//
// Streaming protocol:
//   stream_start  → create bubble, show typing, disable input
//   stream_token  → append token text to bubble
//   stream_done   → finalize, re-enable input
//   stream_error  → show error, re-enable input
//
// This file exists as a standalone reference and for future use
// when the HTML is split into separate asset files served via
// a custom protocol.

'use strict';

const messagesEl = document.getElementById('messages');
const inputEl    = document.getElementById('user-input');
const sendBtn    = document.getElementById('send-btn');

var isStreaming = false;
var currentStreamEl = null;

/**
 * Add a message bubble to the chat.
 * @param {'user'|'alpha'} role
 * @param {string} text
 * @returns {HTMLElement} The text element inside the bubble.
 */
function addMessage(role, text) {
    var msg = document.createElement('div');
    msg.className = 'message ' + role;

    var roleEl = document.createElement('div');
    roleEl.className = 'role';
    roleEl.textContent = role === 'user' ? 'You' : 'Alpha';

    var textEl = document.createElement('div');
    textEl.className = 'text';
    textEl.textContent = text;

    msg.appendChild(roleEl);
    msg.appendChild(textEl);
    messagesEl.appendChild(msg);
    messagesEl.scrollTop = messagesEl.scrollHeight;
    return textEl;
}

/**
 * Enable or disable the input area.
 * @param {boolean} enabled
 */
function setInputEnabled(enabled) {
    inputEl.disabled = !enabled;
    sendBtn.disabled = !enabled;
    sendBtn.style.opacity = enabled ? '1' : '0.5';
}

// ── IPC: Receive streaming events from Rust ──

/**
 * Callback invoked by Rust via webview.evaluate_script().
 * Handles streaming protocol events.
 * @param {{ type: string, payload: object }} msg
 */
window.__alpha_receive = function(msg) {
    if (!msg || !msg.type) return;

    switch (msg.type) {
        case 'stream_start':
            isStreaming = true;
            setInputEnabled(false);
            currentStreamEl = addMessage('alpha', '...');
            break;

        case 'stream_token':
            if (currentStreamEl && msg.payload && msg.payload.token) {
                if (currentStreamEl.textContent === '...') {
                    currentStreamEl.textContent = '';
                }
                currentStreamEl.textContent += msg.payload.token;
                messagesEl.scrollTop = messagesEl.scrollHeight;
            }
            break;

        case 'stream_done':
            isStreaming = false;
            currentStreamEl = null;
            setInputEnabled(true);
            inputEl.focus();
            break;

        case 'stream_error':
            isStreaming = false;
            var errorMsg = (msg.payload && msg.payload.error) || 'Unknown error';
            if (currentStreamEl) {
                currentStreamEl.textContent = 'Error: ' + errorMsg;
                currentStreamEl.parentElement.style.borderLeft = '2px solid #f7768e';
            } else {
                addMessage('alpha', 'Error: ' + errorMsg);
            }
            currentStreamEl = null;
            setInputEnabled(true);
            inputEl.focus();
            break;
    }
};

// ── IPC: Send to Rust ──

/**
 * Handle send action — display user message and send via IPC.
 */
function sendMessage() {
    var text = inputEl.value.trim();
    if (!text || isStreaming) return;

    addMessage('user', text);
    inputEl.value = '';

    var request = JSON.stringify({
        type: 'send_message',
        payload: { message: text }
    });

    if (window.ipc && window.ipc.postMessage) {
        window.ipc.postMessage(request);
    } else {
        addMessage('alpha', 'IPC not available. Running in standalone mode.');
    }
}

// ── Event Listeners ──

sendBtn.addEventListener('click', sendMessage);

inputEl.addEventListener('keydown', function(e) {
    if (e.key === 'Enter' && !e.shiftKey) {
        e.preventDefault();
        sendMessage();
    }
});

console.log('[Alpha] Frontend initialized with streaming IPC.');
