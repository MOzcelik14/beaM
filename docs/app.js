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
});
