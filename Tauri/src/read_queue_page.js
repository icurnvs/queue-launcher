({
    url: location.href,
    text: document.body?.innerText ?? '',
    errorId: document.body?.getAttribute('data-errorid') ?? '',
    challengeVisible: Array.from(document.querySelectorAll('iframe')).some(frame =>
        /recaptcha/i.test(frame.title) && frame.getClientRects().length > 0 &&
        getComputedStyle(frame).visibility === 'visible'),
})
