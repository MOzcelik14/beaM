document.addEventListener('DOMContentLoaded', () => {
  const langEnBtns = document.querySelectorAll('.lang-btn-en');
  const langTrBtns = document.querySelectorAll('.lang-btn-tr');
  const enElements = document.querySelectorAll('.content-en');
  const trElements = document.querySelectorAll('.content-tr');

  function setLanguage(lang) {
    if (lang === 'tr') {
      enElements.forEach(el => el.style.display = 'none');
      trElements.forEach(el => el.style.display = '');
      langTrBtns.forEach(b => b.classList.add('active'));
      langEnBtns.forEach(b => b.classList.remove('active'));
      document.documentElement.lang = 'tr';
      localStorage.setItem('beam_lang', 'tr');
      if (window.location.hash !== '#tr' && !window.location.pathname.endsWith('tr.html')) {
        history.replaceState(null, null, '#tr');
      }
    } else {
      trElements.forEach(el => el.style.display = 'none');
      enElements.forEach(el => el.style.display = '');
      langEnBtns.forEach(b => b.classList.add('active'));
      langTrBtns.forEach(b => b.classList.remove('active'));
      document.documentElement.lang = 'en';
      localStorage.setItem('beam_lang', 'en');
      if (window.location.hash === '#tr') {
        history.replaceState(null, null, '#en');
      }
    }
  }

  langEnBtns.forEach(btn => {
    btn.addEventListener('click', () => setLanguage('en'));
  });

  langTrBtns.forEach(btn => {
    btn.addEventListener('click', () => setLanguage('tr'));
  });

  // Determine initial language from hash, pathname or localStorage
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
