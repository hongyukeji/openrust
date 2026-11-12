import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { serve } from "vitepress";

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.DOCS_PLAYWRIGHT_MODULE || "playwright");
const root = fileURLToPath(new URL("../", import.meta.url));
const browser = await chromium.launch({ executablePath: process.env.DOCS_BROWSER_PATH });
try {
    for (const mount of ["/", "/docs/"]) {
        const { server } = await serve({ root, base: mount, port: 0 });
        try {
            for (const locale of ["en", "zh-hans", "zh-hant"]) {
                const page = await browser.newPage();
                try {
                    const url = `http://127.0.0.1:${server.address().port}${mount}${locale}/digging-deeper/queues.html`;
                    const response = await page.goto(url);
                    assert.equal(response.status(), 200);
                    for (const anchor of ["queue-workers-and-deployment", "listening-with-a-fresh-worker-process", "selecting-jobs-for-a-named-worker", "workers-during-maintenance", "worker-status-and-exit-reasons", "worker-lifecycle-events", "registering-worker-callbacks", "synchronous-job-events", "job-reservation-events", "job-attempt-events", "worker-rest-and-idle-timeout", "worker-memory-limit", "worker-lost-connection", "max-exceptions", "counting-worker-crashes", "stopping-retries-by-exception", "job-expirations-and-timeouts", "hard-job-timeouts", "rate-limiting", "cleaning-up-after-failed-jobs", "retrying-failed-jobs", "refreshing-a-retry-deadline", "retrying-a-group-of-stored-failures", "pruning-failed-jobs", "clearing-jobs-from-queues", "monitoring-your-queues", "preventing-job-overlaps", "throttling-exceptions", "releasing-jobs", "skipping-jobs", "reacting-to-worker-signals", "notifying-a-running-job", "pausing-and-resuming-queue-workers", "pausing-queues-in-code"]) {
                        assert.equal(await page.locator(`#${anchor}`).count(), 1);
                        assert.ok(await page.locator(`a[href="#${anchor}"]`).count());
                    }
                    const body = await page.locator("main").innerText();
                    for (const command of ["queue:restart", "queue:work --json --stop-when-empty", "queue:work --json --once --tries=2", "queue:work --json --rest=1 --stop-when-empty-for=2", "queue:pause database:default", "queue:continue database:default", "queue:pause --all", "queue:resume --all"]) {
                        assert.ok(body.includes(`./artisan ${command}`), `${locale}: ${command}`);
                    }
                    assert.ok(body.includes("Queue::pauseFor"));
                    assert.ok(body.includes("WorkerQueueResumed"));
                    assert.ok(body.includes("restart_signal"));
                    assert.ok(body.includes("empty_for"));
                    assert.ok(body.includes("WorkerIdle"));
                    assert.ok(body.includes("Queue::popUsing"));
                    assert.ok(body.includes("./artisan queue:work database --once --force"));
                    assert.ok(body.includes("./artisan queue:listen database --queue=default --timeout=60"));
                    assert.ok(body.includes("Looping"));
                    assert.ok(body.includes("JobPopping"));
                    assert.ok(body.includes("JobPopped"));
                    for (const event of ["JobProcessing", "JobProcessed", "JobReleased", "JobExceptionOccurred", "JobReleasedAfterException", "JobFailed", "JobTimedOut", "JobAttempted"]) {
                        assert.ok(body.includes(event), `${locale}: ${event}`);
                    }
                    for (const signal of ["USR2", "CONT", "TERM"]) {
                        assert.ok(body.includes(`kill -s ${signal} "$WORKER_PID"`));
                    }
                    assert.ok(body.includes("WorkerInterrupted"));
                    assert.ok(body.includes("max_exceptions: Some(3)"));
                    assert.ok(body.includes("count_crashes_as_exceptions: true"));
                    assert.ok(body.includes("dontRetryWhen"));
                    assert.ok(body.includes("./artisan queue:clear database --queue=emails,reports"));
                    assert.ok(body.includes("./artisan queue:prune-failed --hours=48"));
                    assert.ok(body.includes("JobHandler::failed"));
                    assert.ok(body.includes("Queue::failing"));
                    assert.ok(body.includes("Queue::whenJobRunsLongerThan"));
                    assert.ok(body.includes("register_sync_observer"));
                    assert.ok(body.includes("./artisan queue:retry --queue=emails"));
                    assert.ok(body.includes("JobRetryRequested"));
                    assert.ok(body.includes("fn retryUntil"));
                    assert.ok(body.includes("FailedJobRetryStream"));
                    assert.ok(body.includes("ClearCommand::prohibit(true)"));
                    assert.ok(body.includes("./artisan queue:monitor database:emails,reports --max=100 --json"));
                    assert.ok(body.includes("QueueBusy"));
                    assert.ok(body.includes("oldest_pending"));
                    const configuration = await page.goto(`http://127.0.0.1:${server.address().port}${mount}${locale}/getting-started/configuration.html`);
                    assert.equal(configuration.status(), 200);
                    assert.equal(await page.locator("#maintenance-mode").count(), 1);
                    assert.equal(await page.locator("#shared-maintenance-state").count(), 1);
                    const configurationBody = await page.locator("main").innerText();
                    for (const term of ["maintenanceMode()", "./artisan down --retry=60 --with-secret", "./artisan up", "preventRequestsDuringMaintenance", "$retryAfter"]) {
                        assert.ok(configurationBody.includes(term), `${locale}: ${term}`);
                    }
                    assert.equal(await page.locator('main a[href$="#workers-during-maintenance"]').count(), 1);
                    const errors = await page.goto(`http://127.0.0.1:${server.address().port}${mount}${locale}/the-basics/errors.html`);
                    assert.equal(errors.status(), 200);
                    const queueLink = page.locator('main a[href$="#stopping-retries-by-exception"]');
                    assert.equal(await queueLink.count(), 1);
                    await queueLink.click();
                    await page.waitForURL(/queues(?:\.html)?#stopping-retries-by-exception$/);
                    await page.locator("#stopping-retries-by-exception").waitFor({ state: "attached" });
                    for (const term of ["hard_timeout: true", "--timeout=60 --tries=2"]) {
                        assert.ok(body.includes(term), `${locale}: ${term}`);
                    }
                    for (const term of ["RateLimiter::forJob", "RateLimitedWithRedis", "dontRelease", "releaseAfter"]) {
                        assert.ok(body.includes(term), `${locale}: ${term}`);
                    }
                    for (const term of ["WithoutOverlapping", "shared()", "withPrefix", "expireAfter"]) {
                        assert.ok(body.includes(term), `${locale}: ${term}`);
                    }
                    for (const term of ["ThrottlesExceptions", "ThrottlesExceptionsWithRedis", "ExceptionThrottleLimiter", "deleteWhen", "failWhen"]) {
                        assert.ok(body.includes(term), `${locale}: ${term}`);
                    }
                    for (const term of ["Release::when", "Release::unless", "Duration::ZERO", "Skip::when", "Skip::unless"]) {
                        assert.ok(body.includes(term), `${locale}: ${term}`);
                    }
                    for (const term of ["timed_out", "QUEUE_WORKER_TIMED_OUT_EXIT_CODE=124", "QUEUE_WORKER_KILL_ON_TIMEOUT=false"]) {
                        assert.ok(body.includes(term), `${locale}: ${term}`);
                    }
                    for (const term of ["lost_connection", "QUEUE_WORKER_STOP_ON_LOST_CONNECTION=false", "LostConnectionDetector"]) {
                        assert.ok(body.includes(term), `${locale}: ${term}`);
                    }
                    for (const term of ["--memory=128", "--memory=50%", "QUEUE_WORKER_MEMORY_LIMIT", "12"]) {
                        assert.ok(body.includes(term), `${locale}: ${term}`);
                    }
                    for (const term of ["JobInterrupted", "Interruptible", "ProcessPodcast"]) {
                        assert.ok(body.includes(term), `${locale}: ${term}`);
                    }
                    console.log(mount, locale, "PASS");
                } finally {
                    await page.close();
                }
            }
        } finally {
            await promisify(server.close).call(server);
        }
    }
} finally {
    await browser.close();
}
