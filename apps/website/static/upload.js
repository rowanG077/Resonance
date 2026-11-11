const form = document.querySelector('#mod-upload-form');
if (form) {
  const fileInput = document.querySelector('#archive');
  const submit = document.querySelector('#upload-submit');
  const cancel = document.querySelector('#upload-cancel');
  const status = document.querySelector('#upload-status');
  const progress = document.querySelector('#upload-progress');
  const notice = document.querySelector('#resume-notice');
  const csrf = form.elements.csrf.value;
  let upload = null;
  let busy = false;
  let cancelling = false;
  let controller = null;
  class ApiError extends Error { constructor(status, message) { super(message); this.status = status; } }
  async function api(path, { method = 'GET', body, signal } = {}) {
    const headers = { 'X-CSRF-Token': csrf };
    if (body && !(body instanceof Blob)) { headers['Content-Type'] = 'application/json'; body = JSON.stringify(body); }
    const response = await fetch(path, { method, body, headers, signal });
    const data = await response.json().catch(error => {
      if (response.ok) throw error;
      return { error: `Request failed (${response.status}).` };
    });
    if (!response.ok) throw new ApiError(response.status, data.error || 'Request failed.');
    return data;
  }
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  function showProgress() {
    if (!upload) { progress.hidden = true; return; }
    progress.hidden = false;
    progress.max = upload.size;
    progress.value = upload.received;
    status.textContent = `${(upload.received / 1048576).toFixed(1)} / ${(upload.size / 1048576).toFixed(1)} MiB uploaded.`;
  }
  function setBusy(value) {
    busy = value || cancelling;
    for (const control of form.querySelectorAll('input, select, textarea')) control.disabled = busy || Boolean(upload && control !== fileInput);
    submit.disabled = busy;
    cancel.hidden = !upload;
  }
  function restoreUpload(data) {
    upload = data;
    if (!upload) { submit.textContent = 'Submit for review'; cancel.hidden = true; return; }
    for (const [key, value] of Object.entries(upload.metadata)) {
      const control = form.elements.namedItem(key);
      if (!control) continue;
      if (control.type === 'checkbox') control.checked = Boolean(value);
      else control.value = value ?? '';
    }
    notice.hidden = false;
    notice.textContent = upload.received === upload.size
      ? 'Your file is uploaded. Finish the upload to submit it for review.'
      : 'An unfinished upload is saved. Select the original ZIP to resume it, or cancel it to start another.';
    submit.textContent = upload.received === upload.size ? 'Finish upload' : 'Resume upload';
    fileInput.required = upload.received !== upload.size;
    cancel.hidden = false;
    showProgress();
    setBusy(false);
  }
  async function fingerprint(file, partSize) {
    const hex = bytes => Array.from(new Uint8Array(bytes), b => b.toString(16).padStart(2, '0')).join('');
    const first = hex(await crypto.subtle.digest('SHA-256', await file.slice(0, partSize).arrayBuffer()));
    const lastAt = Math.floor((file.size - 1) / partSize) * partSize;
    const last = hex(await crypto.subtle.digest('SHA-256', await file.slice(lastAt).arrayBuffer()));
    return first + last;
  }
  function metadata() {
    const values = Object.fromEntries(new FormData(form));
    return {
      mod_id: values.mod_id || null, title: values.title, summary: values.summary,
      description: values.description, category: values.category, version: values.version,
      resonance_version: values.resonance_version, content_version: Number(values.content_version),
      rights: values.rights === 'yes',
    };
  }
  async function finish(signal) {
    status.textContent = 'Checking the archive before submitting it for review…';
    for (let attempt = 0; attempt < 120; attempt++) {
      if (signal.aborted) throw new DOMException('Upload cancelled.', 'AbortError');
      try {
        const path = `/api/v1/uploads/${upload.id}`;
        upload = await (upload.status === 'uploading'
          ? api(`${path}/complete`, { method: 'POST', signal })
          : api(path, { signal }));
      }
      catch (error) {
        if (error.name === 'AbortError' || error instanceof ApiError && error.status !== 409 && error.status !== 429 && error.status < 500) throw error;
        // A lost completion response may still have started validation.
        upload = await api(`/api/v1/uploads/${upload.id}`, { signal });
      }
      if (upload.status === 'submitted') return;
      if (['failed', 'aborted'].includes(upload.status)) throw new Error(upload.error || 'The upload could not be submitted. Cancel it and start again.');
      await sleep(Math.min(1000 * (attempt + 1), 10000));
    }
    throw new Error('The server is still finishing this upload. Retry in a few minutes.');
  }
  form.addEventListener('submit', async event => {
    event.preventDefault();
    if (busy) return;
    const file = fileInput.files[0];
    const draft = metadata();
    if ((!upload || upload.received < upload.size) && !file) { status.textContent = 'Select the ZIP file.'; return; }
    controller = new AbortController();
    const signal = controller.signal;
    setBusy(true);
    try {
      if (!upload) {
        status.textContent = 'Starting the upload…';
        const id = await fingerprint(file, 8 * 1048576);
        try {
          upload = await api('/api/v1/uploads', { method: 'POST', body: { metadata: draft, size: file.size, fingerprint: id }, signal });
        } catch (error) {
          if (error.name === 'AbortError' || error instanceof ApiError && error.status !== 409 && error.status < 500) throw error;
          // A failed response can still leave a committed reservation on the server.
          const saved = (await api('/api/v1/uploads/active', { signal })).upload;
          if (!saved) throw error;
          restoreUpload(saved);
          status.textContent = 'An unfinished upload was recovered. Review its details, then resume it or cancel it.';
          return;
        }
      } else if (upload.received < upload.size && (file.size !== upload.size || await fingerprint(file, upload.part_size) !== upload.fingerprint)) {
        throw new Error('Select the original ZIP to resume this upload, or cancel it and start another.');
      }
      cancel.hidden = false;
      showProgress();
      let retries = 0;
      while (upload.received < upload.size) {
        if (signal.aborted) throw new DOMException('Upload cancelled.', 'AbortError');
        const part = upload.next_part;
        const from = (part - 1) * upload.part_size;
        try {
          upload = await api(`/api/v1/uploads/${upload.id}/parts/${part}`, { method: 'PUT', body: file.slice(from, from + upload.part_size), signal });
          retries = 0;
          showProgress();
        } catch (error) {
          if (error.name === 'AbortError' || error instanceof ApiError && ![409,429].includes(error.status) && error.status < 500 || retries++ >= 20) throw error;
          status.textContent = 'Upload interrupted. Checking saved progress before retrying…';
          await sleep(Math.min(retries * 1000, 10000));
          upload = await api(`/api/v1/uploads/${upload.id}`, { signal });
          if (upload.status !== 'uploading') throw new Error('This upload is no longer accepting file parts. Refresh the page to check its status.');
        }
      }
      await finish(signal);
      location.assign(upload.page_url);
    } catch (error) {
      if (error.name !== 'AbortError') status.textContent = error.message;
      if (upload) submit.textContent = upload.received === upload.size ? 'Finish upload' : 'Resume upload';
    } finally { setBusy(false); }
  });
  cancel.addEventListener('click', async () => {
    if (!upload) return;
    cancelling = true;
    controller?.abort();
    cancel.disabled = true;
    try {
      await api(`/api/v1/uploads/${upload.id}/abort`, { method: 'POST' });
      // Reload the original page so restored metadata cannot change its mod destination.
      location.reload();
    } catch (error) { status.textContent = error.message; }
    finally { cancelling = false; cancel.disabled = false; setBusy(false); }
  });
  api('/api/v1/uploads/active').then(data => {
    restoreUpload(data.upload);
    submit.disabled = false;
  }).catch(error => { status.textContent = error.message; });
}
