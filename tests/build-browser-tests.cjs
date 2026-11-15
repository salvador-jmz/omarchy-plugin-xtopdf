const fs = require('node:fs');
const path = require('node:path');
const extract = fs.readFileSync(path.join(__dirname, '../assets/extract.js'), 'utf8');
const tests = String.raw`
const results = [];
function test(name, body) { try { body(); results.push('PASS ' + name); } catch(e) {results.push('FAIL ' + name + ': ' + e.message)} }
function assert(ok, message) { if (!ok) throw new Error(message) }
function set(html) { document.querySelector('#fixture').innerHTML = html; }
function block(text) {return '<div data-block="true">' + text + '</div>'}
for (const selector of ['twitterArticleReadView','twitterArticleRichText','articleNoteTweet','longformRichTextComponent']) {
 test('extracts ' + selector, () => {set('<article><div data-testid="' + selector + '"><h1>Title</h1>' + block('First') + block('Second') + '</div></article>'); assert(extractArticle().article.content.length === 2, 'Expected two blocks'); });
}
test('preserves code order and repeated paragraphs', () => {
 set('<article data-testid="twitterArticleReadView"><h1>Title</h1>' + block('Before') + '<div data-testid="markdown-code-block"><pre><span data-block="true">let answer = 42;</span></pre></div>' + block('After') + block('After') + '</article>');
 const a=extractArticle().article; assert(a.content.length===4, 'Duplicate or missing block'); assert(a.content[1].kind==='code', 'Code moved'); assert(a.content[2].runs[0].text==='After' && a.content[3].runs[0].text==='After', 'Repeated text lost');
});
test('preserves semantic list and heading wrappers', () => {
 set('<article data-testid="twitterArticleReadView"><h2>'+block('Heading')+'</h2><ol><li>'+block('One')+'</li><li>'+block('Two')+'</li></ol><blockquote>'+block('Quote')+'</blockquote></article>');
 const c=extractArticle().article.content; assert(c[0].kind==='heading'&&c[0].level===2,'Heading lost'); assert(c[1].kind==='list-item'&&c[1].ordered,'Ordered list lost'); assert(c[3].kind==='quote','Quote lost');
});
test('does not export markup or event handlers', () => {
 set('<article data-testid="twitterArticleReadView">'+block('<strong onclick="evil()">Bold</strong><img src="data:," onerror="void 0"><script>evil()</scr'+'ipt><a href="javascript:evil()">Link</a>')+'</article>');
 const c=extractArticle().article.content; const text=JSON.stringify(c); assert(!text.includes('onclick')&&!text.includes('onerror')&&!text.includes('<img'),'Attributes leaked'); assert(c[0].runs[0].kind==='strong','Formatting lost'); assert(!text.includes('"text":"evil()"'),'Script text leaked');
});
test('nested wrappers are not duplicated', () => {set('<article data-testid="twitterArticleReadView"><div data-block="true">'+block('Once')+'</div></article>'); assert(extractArticle().article.content.length===1,'Duplicate nested wrapper');});
test('article title outside the body is used once', () => {
 set('<article><div data-testid="twitterArticleReadView"><div data-testid="twitter-article-title"><div data-block="true">Real Title</div></div><div data-testid="twitterArticleRichText">'+block('Body')+'</div></div></article>');
 const a=extractArticle().article; assert(a.title==='Real Title','Title missed: '+a.title); assert(a.content.length===1&&a.content[0].runs[0].text==='Body','Title duplicated in body');
});
test('document title fallback is the article title, not the author', () => {
 const saved=document.title; document.title='Some Author on X: "Fallback Title" / X';
 try { set('<article data-testid="twitterArticleReadView">'+block('Body')+'</article>'); const t=extractArticle().article.title; assert(t==='Fallback Title','Got '+t); }
 finally { document.title=saved; }
});
test('article images are kept in reading order; chrome images are not', () => {
 const img=(src,alt)=>'<img src="'+src+'" alt="'+(alt||'')+'">';
 set('<article data-testid="twitterArticleReadView"><div data-testid="UserAvatar-Container-x">'+img('https://pbs.twimg.com/profile_images/1/a.jpg')+'</div>'+block('Before')+'<section data-block="true"><div data-testid="tweetPhoto">'+img('https://pbs.twimg.com/media/ABC?format=jpg&name=small','Chart')+img('https://pbs.twimg.com/media/ABC?format=jpg&name=medium')+'</div></section>'+block('After')+img('https://abs.twimg.com/emoji/v2/svg/1f600.svg')+img('https://pbs.twimg.com/emoji/x.png')+img('https://evil.test/media/x.jpg')+'</article>');
 const c=extractArticle().article.content; assert(c.length===3,'Expected 3 blocks, got '+JSON.stringify(c)); assert(c[1].kind==='image'&&c[1].src.startsWith('https://pbs.twimg.com/media/ABC')&&c[1].alt==='Chart','Image lost'); assert(c[2].runs[0].text==='After','Order lost'); assert(!('data' in c[1]),'Page script must not supply image data');
});
test('image-only page is not an article', () => {set('<article data-testid="twitterArticleReadView"><img src="https://pbs.twimg.com/media/A?name=small"></article>'); assert(!!extractArticle().error,'Image-only accepted');});
test('ordinary posts are rejected', () => {set('<article><div data-testid="tweetText">Post</div></article>'); assert(!!extractArticle().error,'Post accepted');});
test('empty article is rejected', () => {set('<div data-testid="twitterArticleReadView"></div>'); assert(!!extractArticle().error,'Empty accepted');});
document.querySelector('#fixture').remove();
const failed=results.some(r=>r.startsWith('FAIL'));
document.querySelector('#results').textContent = (failed?'FAILED':'ALL PASSED') + '\n' + results.join('\n');
document.title=failed?'XtoPDF tests FAILED':'XtoPDF tests ALL PASSED';
`;
const html = '<!doctype html><meta charset="utf-8"><title>XtoPDF extraction tests</title><h1>XtoPDF · DOM extraction tests</h1><pre id="results">Running</pre><div id="fixture"></div><script>function extractArticle(){return (\n' + extract + '\n);}\n' + tests + '</script>';
fs.writeFileSync(process.argv[2], html);
