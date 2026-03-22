// Copy buttons on code blocks
(function() {
	document.querySelectorAll('pre').forEach(function(pre) {
		var copyIcon = '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>';
		var checkIcon = '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></svg>';
		var btn = document.createElement('button');
		btn.className = 'copy-btn';
		btn.innerHTML = copyIcon;
		btn.addEventListener('click', function() {
			var code = pre.querySelector('code');
			var text = (code || pre).textContent;
			navigator.clipboard.writeText(text).then(function() {
				btn.innerHTML = checkIcon;
				btn.classList.add('copied');
				setTimeout(function() {
					btn.innerHTML = copyIcon;
					btn.classList.remove('copied');
				}, 1500);
			});
		});
		pre.appendChild(btn);
	});
})();

// Copy contract hashes from env vars table
(function() {
	document.querySelectorAll('.hash-copy').forEach(function(el) {
		el.addEventListener('click', function(e) {
			e.preventDefault();
			var hash = el.getAttribute('data-hash');
			navigator.clipboard.writeText(hash).then(function() {
				el.classList.add('copied');
				setTimeout(function() {
					el.classList.remove('copied');
				}, 1500);
			}).catch(function(err) {
				console.error('Failed to copy hash:', err);
			});
		});
	});
})();

// Sidebar active state (scroll-spy)
(function() {
	var links = document.querySelectorAll('.sidebar a[href^="#"]');
	var entries = [];
	links.forEach(function(link) {
		var id = link.getAttribute('href').slice(1);
		var el = document.getElementById(id);
		if (el) entries.push({ id: id, el: el, link: link });
	});

	function setActive(link) {
		links.forEach(function(l) { l.classList.remove('active'); });
		if (link) link.classList.add('active');
	}

	var current = null;
	var observer = new IntersectionObserver(function(changes) {
		changes.forEach(function(entry) {
			if (entry.isIntersecting) {
				var match = null;
				for (var i = 0; i < entries.length; i++) {
					if (entries[i].el === entry.target) { match = entries[i]; break; }
				}
				if (match) {
					current = match.link;
					setActive(current);
				}
			}
		});
	}, {
		rootMargin: '-80px 0px -65% 0px',
		threshold: 0
	});

	entries.forEach(function(e) { observer.observe(e.el); });
})();
