import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {fileURLToPath} from 'node:url';
import {serve} from 'vitepress';
const require=createRequire(import.meta.url);
const {chromium}=require(process.env.DOCS_PLAYWRIGHT_MODULE);
const root=process.env.ARTISAN_DOCS_ROOT || fileURLToPath(new URL('../',import.meta.url));
const browser=await chromium.launch({executablePath:process.env.DOCS_BROWSER_PATH});
try {
    for (const base of ['/','/docs/']) {
        const {server}=await serve({root,base,port:0});
        try {
            for (const locale of ['en','zh-Hans','zh-Hant']) {
                for (const width of [375,1440]) {
                    const page=await browser.newPage({viewport:{width,height:900}});
                    const errors=[];page.on('pageerror',error=>errors.push(error.message));
                    const response=await page.goto(`http://127.0.0.1:${server.address().port}${base}${locale}/digging-deeper/artisan.html#command-signatures`);
                    assert.equal(response.status(),200);
                    await page.locator('#command-signatures').scrollIntoViewIfNeeded();
                    assert.equal(await page.locator('h2#command-signatures').count(),1);
                    assert.equal(await page.locator('h2#typed-command-input').count(),1);
                    assert.equal(await page.locator('h2#isolatable-commands').count(),1);
                    assert.equal(await page.locator('h2#editing-command-input').count(),1);
                    assert.equal(await page.locator('h2#prompting-for-missing-input').count(),1);
                    assert.equal(await page.locator('h2#command-lifecycle').count(),1);
                    assert.equal(await page.locator('h2#writing-command-output').count(),1);
                    assert.equal(await page.locator('h2#output-style-verbosity').count(),1);
                    assert.equal(await page.locator('h2#interactive-questions').count(),1);
                    assert.equal(await page.locator('h2#choice-questions').count(),1);
                    assert.equal(await page.locator('h2#tables').count(),1);
                    assert.equal(await page.locator('h2#progress-bars').count(),1);
                    assert.equal(await page.locator('h2#lazy-progress-iteration').count(),1);
                    assert.equal(await page.locator('h2#async-progress-callbacks').count(),1);
                    assert.equal(await page.locator('h2#progress-placeholder-output').count(),1);
                    assert.equal(await page.locator('h2#output-configuration').count(),1);
                    assert.equal(await page.locator('h2#formatter-styles').count(),1);
                    assert.equal(await page.locator('h2#output-formatters').count(),1);
                    assert.equal(await page.locator('h2#colors-terminal').count(),1);
                    assert.equal(await page.locator('h2#table-layout-controls').count(),1);
                    assert.equal(await page.locator('h2#table-rows-orientation').count(),1);
                    assert.equal(await page.locator('h2#table-style-definitions').count(),1);
                    assert.equal(await page.locator('h2#table-row-separators').count(),1);
                    assert.equal(await page.locator('h2#table-cell-styles').count(),1);
                    assert.equal(await page.locator('h2#table-cell-spans').count(),1);
                    assert.equal(await page.locator('h2#console-sections').count(),1);
                    assert.equal(await page.locator('h2#section-consumers').count(),1);
                    assert.equal(await page.locator('h2#section-editors').count(),1);
                    assert.equal(await page.locator('h2#nested-output-styles').count(),1);
                    assert.equal(await page.locator('h2#style-text-layout').count(),1);
                    assert.equal(await page.locator('h2#styled-message-blocks').count(),1);
                    assert.equal(await page.locator('h2#style-question-helpers').count(),1);
                    assert.equal(await page.locator('h2#table-output-factory').count(),1);
                    assert.equal(await page.locator('h2#style-table-helpers').count(),1);
                    assert.equal(await page.locator('h2#style-interface').count(),1);
                    assert.equal(await page.locator('h2#error-style').count(),1);
                    assert.equal(await page.locator('h2#tree-nodes').count(),1);
                    assert.equal(await page.locator('h2#trees').count(),1);
                    assert.equal(await page.locator('h2#component-mutators').count(),1);
                    assert.equal(await page.locator('h2#line-components').count(),1);
                    assert.equal(await page.locator('h2#display-components').count(),1);
                    assert.equal(await page.locator('h2#task-results').count(),1);
                    assert.equal(await page.locator('h2#task-components').count(),1);
                    assert.equal(await page.locator('h2#question-helpers').count(),1);
                    assert.equal(await page.locator('h2#question-components').count(),1);
                    assert((await page.locator('.vp-doc').innerText()).includes('register_display_components'));
                    assert((await page.locator('.vp-doc').innerText()).includes('register_line_components'));
                    assert((await page.locator('.vp-doc').innerText()).includes('transform_component_text'));
                    assert((await page.locator('.vp-doc').innerText()).includes('register_tree_output'));
                    assert((await page.locator('.vp-doc').innerText()).includes('register_tree_nodes'));
                    const text=await page.locator('.vp-doc').innerText();
                    for (const snippet of ['command_with','from_signature','Arc<Mailer>','register_injected','{--Q|queue=default}','register_typed','integer_strict','input.input()','register_isolated','add_isolatable','register_edited','set_option','register_prompted','PromptsForMissingInput','CommandRegistration','register_counter','AtomicUsize','register_output','AsyncCommandCallback','output_components','register_styled','OutputFormat::Raw','OutputVerbosity::Quiet','register_questions','io.anticipate','QuestionCallback','register_choices','choice_pairs_with','ChoiceQuestion','register_tables','TablePadding','register_progress','with_progress_bar_iter','ProgressFormatter','register_progress_iteration','progress_iterate','register_progress_async','with_progress_bar_iter_mut_async','register_progress_output','ProgressOutput','register_output_configuration','set_verbosity','register_formatter_styles','OutputFormatterStyleInterface','ResetInterface','register_formatter','WrappableOutputFormatterInterface','register_colors','get_color_mode','AnsiColorMode','register_layout','set_column_max_width','get_style_mut','register_rows','set_vertical','set_row']) assert(text.includes(snippet),`${locale}: ${snippet}`);
                    assert.equal(await page.locator('h3#component-factories').count(),1);
                    assert.equal(await page.locator('h3#dynamic-component-calls').count(),1);
                    assert.equal(await page.locator('h3#custom-console-components').count(),1);
                    assert.equal(await page.locator('h3#component-data-scopes').count(),1);
                    assert.equal(await page.locator('h3#component-markup-compilation').count(),1);
                    assert.equal(await page.locator('h3#component-markup-rendering').count(),1);
                    assert.equal(await page.locator('h3#component-color-breakpoints').count(),1);
                    assert.deepEqual(errors,[]);
                    console.log(`${base} ${locale} ${width}: formal signatures and injected example PASS`);
                    await page.close();
                }
            }
        } finally {await new Promise((resolve,reject)=>server.close(error=>error?reject(error):resolve()));}
    }
} finally {await browser.close();}
