(function () {
  const badge = document.getElementById('version-badge');
  if (!badge) return;

  const base = document.querySelector('.site-url')?.textContent || '';
  const metadataUrl = (base ? base.replace(/\/$/, '') : '') + '/dl/cli/latest.json';

  fetch(metadataUrl)
    .then(function (response) {
      if (!response.ok) throw new Error('metadata unavailable');
      return response.json();
    })
    .then(function (data) {
      if (data.version) {
        badge.textContent = 'v' + data.version.replace(/^v/, '');
      }
    })
    .catch(function () {
      // Leave the badge empty on failure.
    });
})();
