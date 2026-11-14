document.addEventListener('DOMContentLoaded', () => {
  const langEnBtns = document.querySelectorAll('.lang-btn-en');
  const langTrBtns = document.querySelectorAll('.lang-btn-tr');
  const enElements = document.querySelectorAll('.content-en');
  const trElements = document.querySelectorAll('.content-tr');

  function setLanguage(lang) {
    if (lang === 'tr') {
      enElements.forEach(el => { el.style.display = 'none'; });
      trElements.forEach(el => { el.style.display = ''; });
      langTrBtns.forEach(b => b.classList.add('active'));
      langEnBtns.forEach(b => b.classList.remove('active'));
      document.documentElement.lang = 'tr';
      localStorage.setItem('beam_lang', 'tr');
      if (window.location.hash !== '#tr' && !window.location.pathname.endsWith('tr.html')) {
        history.replaceState(null, null, '#tr');
      }
    } else {
      trElements.forEach(el => { el.style.display = 'none'; });
      enElements.forEach(el => { el.style.display = ''; });
      langEnBtns.forEach(b => b.classList.add('active'));
      langTrBtns.forEach(b => b.classList.remove('active'));
      document.documentElement.lang = 'en';
      localStorage.setItem('beam_lang', 'en');
      if (window.location.hash === '#tr') {
        history.replaceState(null, null, '#en');
      }
    }
  }

  langEnBtns.forEach(btn => btn.addEventListener('click', () => setLanguage('en')));
  langTrBtns.forEach(btn => btn.addEventListener('click', () => setLanguage('tr')));

  const currentPath = window.location.pathname;
  const hash = window.location.hash;
  const storedLang = localStorage.getItem('beam_lang');

  if (currentPath.endsWith('tr.html') || hash === '#tr') {
    setLanguage('tr');
  } else if (storedLang === 'tr' && hash !== '#en') {
    setLanguage('tr');
  } else {
    setLanguage('en');
  }

  // Sticky header elevation on scroll
  const header = document.querySelector('.site-header');
  if (header) {
    const updateHeader = () => {
      header.classList.toggle('scrolled', window.scrollY > 16);
    };
    updateHeader();
    window.addEventListener('scroll', updateHeader, { passive: true });
  }

  // Scroll reveal animations
  const revealItems = document.querySelectorAll('[data-reveal]');
  if ('IntersectionObserver' in window && revealItems.length > 0) {
    const observer = new IntersectionObserver((entries, obs) => {
      entries.forEach(entry => {
        if (entry.isIntersecting) {
          entry.target.classList.add('is-visible');
          obs.unobserve(entry.target);
        }
      });
    }, { threshold: 0.12, rootMargin: '0px 0px -30px 0px' });

    revealItems.forEach(el => observer.observe(el));
  } else {
    revealItems.forEach(el => el.classList.add('is-visible'));
  }

  // Interactive Hero Workspace Stage switcher
  const stageTabs = document.querySelectorAll('[data-stage-tab]');
  const stagePanels = document.querySelectorAll('[data-stage-panel]');
  const stageCaptions = document.querySelectorAll('[data-stage-caption]');

  stageTabs.forEach(tab => {
    tab.addEventListener('click', () => {
      const target = tab.getAttribute('data-stage-tab');
      stageTabs.forEach(t => {
        const active = t === tab;
        t.classList.toggle('active', active);
        t.setAttribute('aria-selected', active ? 'true' : 'false');
      });
      stagePanels.forEach(panel => {
        panel.classList.toggle('active', panel.getAttribute('data-stage-panel') === target);
      });
      stageCaptions.forEach(cap => {
        cap.style.display = cap.getAttribute('data-stage-caption') === target ? '' : 'none';
      });
    });
  });

  // Image Lightbox (<dialog>)
  const lightbox = document.getElementById('lightbox-dialog');
  const lightboxImg = document.getElementById('lightbox-img');
  const lightboxTitle = document.getElementById('lightbox-title');
  const lightboxClose = document.getElementById('lightbox-close');

  if (lightbox && lightboxImg && lightboxTitle) {
    document.querySelectorAll('[data-lightbox-src]').forEach(img => {
      img.addEventListener('click', () => {
        lightboxImg.src = img.getAttribute('data-lightbox-src') || img.getAttribute('src');
        lightboxImg.alt = img.getAttribute('alt') || 'beaM. Preview';
        lightboxTitle.textContent = img.getAttribute('alt') || 'beaM. Preview';
        lightbox.showModal();
      });
    });

    if (lightboxClose) {
      lightboxClose.addEventListener('click', () => lightbox.close());
    }
    lightbox.addEventListener('click', e => {
      if (e.target === lightbox) {
        lightbox.close();
      }
    });
  }

  // Copy build commands
  const copyBtn = document.getElementById('copy-build-btn');
  const buildCode = document.getElementById('build-code-text');
  if (copyBtn && buildCode && navigator.clipboard) {
    copyBtn.addEventListener('click', async () => {
      try {
        await navigator.clipboard.writeText(buildCode.innerText.trim());
        const prev = copyBtn.textContent;
        copyBtn.textContent = document.documentElement.lang === 'tr' ? 'Kopyalandi' : 'Copied';
        setTimeout(() => { copyBtn.textContent = prev; }, 1600);
      } catch (_) {
        // Ignore clipboard errors in restricted contexts
      }
    });
  }

  // Sync latest GitHub Release version and URLs
  async function syncLatestRelease() {
    const cacheKey = 'beam_latest_release_v1';
    const applyRelease = (tagName, htmlUrl) => {
      if (!tagName) return;
      const cleanVer = tagName.replace(/^v/i, '');
      const vTag = `v${cleanVer}`;
      document.querySelectorAll('[data-beam-version]').forEach(el => {
        el.textContent = vTag;
      });
      document.querySelectorAll('[data-beam-version-plain]').forEach(el => {
        el.textContent = cleanVer;
      });
      if (htmlUrl) {
        document.querySelectorAll('[data-beam-release-url]').forEach(el => {
          el.setAttribute('href', htmlUrl);
        });
      }
    };

    try {
      const cachedRaw = sessionStorage.getItem(cacheKey);
      if (cachedRaw) {
        const cached = JSON.parse(cachedRaw);
        if (cached && Date.now() - cached.ts < 600000) {
          applyRelease(cached.tag, cached.url);
          return;
        }
      }
      const res = await fetch('https://api.github.com/repos/MOzcelik14/beaM/releases/latest', {
        headers: { 'Accept': 'application/vnd.github+json' }
      });
      if (!res.ok) return;
      const data = await res.json();
      if (data && data.tag_name) {
        applyRelease(data.tag_name, data.html_url);
        sessionStorage.setItem(cacheKey, JSON.stringify({
          tag: data.tag_name,
          url: data.html_url,
          ts: Date.now()
        }));
      }
    } catch (_) {
      // Offline or rate-limited: keep static fallback version
    }
  }

  syncLatestRelease();
});
