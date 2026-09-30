// Minimal HTMX-compatible SSE connector (~1.5KB) for streaming DOM swaps
(function() {
  function initSSE() {
    var elements = document.querySelectorAll('[hx-ext="sse"]');
    elements.forEach(function(el) {
      var url = el.getAttribute('sse-connect');
      if (!url) return;

      var evtSource = new EventSource(url);

      evtSource.onopen = function() {
        var statusEl = document.getElementById('sse-status-badge');
        if (statusEl) {
          statusEl.className = 'badge badge-emerald';
          statusEl.textContent = 'CONNECTED';
        }
      };

      evtSource.onerror = function() {
        var statusEl = document.getElementById('sse-status-badge');
        if (statusEl) {
          statusEl.className = 'badge badge-rose';
          statusEl.textContent = 'DISCONNECTED';
        }
      };

      var targets = el.querySelectorAll('[sse-swap]');
      targets.forEach(function(targetEl) {
        var eventName = targetEl.getAttribute('sse-swap');
        var swapMode = targetEl.getAttribute('hx-swap') || 'outerHTML';

        evtSource.addEventListener(eventName, function(e) {
          if (swapMode === 'afterbegin') {
            targetEl.insertAdjacentHTML('afterbegin', e.data);
            // Limit stream to 50 rows
            while (targetEl.children.length > 50) {
              targetEl.removeChild(targetEl.lastChild);
            }
          } else if (swapMode === 'innerHTML') {
            targetEl.innerHTML = e.data;
          } else {
            var temp = document.createElement('div');
            temp.innerHTML = e.data.trim();
            if (temp.firstElementChild) {
              targetEl.parentNode.replaceChild(temp.firstElementChild, targetEl);
            }
          }
        });
      });
    });
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', initSSE);
  } else {
    initSSE();
  }
})();
