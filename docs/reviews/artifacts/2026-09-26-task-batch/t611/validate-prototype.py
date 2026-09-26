import json
from pathlib import Path
from playwright.sync_api import sync_playwright
results=[]
with sync_playwright() as p:
    browser=p.chromium.launch(executable_path='/usr/bin/google-chrome',headless=True,args=['--no-sandbox'])
    for width,height,device in [(380,560,'Computer'),(960,360,'Tablet'),(1280,800,'Tablet')]:
        page=browser.new_page(viewport={'width':width,'height':height},device_scale_factor=1)
        page.goto((Path(__file__).resolve().parent/'flows.html').as_uri())
        page.locator('#device').select_option(device)
        assert page.locator('#camera').inner_text()=='Camera sharing is off.'
        assert page.locator('#display').is_disabled()
        page.locator('#start').click()
        assert 'Connect' in page.locator('#notice').inner_text()
        page.locator('#pair').click();page.locator('#display').click()
        page.locator('#apply').click();assert page.locator('#stop').is_disabled()
        page.locator('#start').click();assert 'Allow camera access' in page.locator('#notice').inner_text()
        page.locator('#permission').check();page.locator('#start').click();page.locator('#stop').click()
        assert page.locator('#display').inner_text()=='Stop display'
        page.get_by_text('Advanced video settings',exact=True).click()
        assert page.get_by_text('More workers can increase CPU, memory and latency.',exact=True).is_visible()
        page.get_by_text('App & diagnostics',exact=True).click();page.locator('#unsupported').check()
        assert page.locator('#start').is_disabled() and page.locator('#display').is_disabled()
        assert page.evaluate('document.documentElement.scrollWidth<=innerWidth')
        page.locator('#unsupported').uncheck();page.evaluate('scrollTo(0,0)')
        page.screenshot(path=str(Path(__file__).resolve().parent/f'prototype-{width}.png'),full_page=True)
        results.append({'viewport':[width,height],'device':device,'tasks':'pair, start display, apply without camera start, permission denial, explicit camera start/stop, advanced settings, unsupported capabilities','horizontal_overflow':False})
        page.close()
    browser.close()
(Path(__file__).resolve().parent/'prototype-validation.json').write_text(json.dumps(results,indent=2)+'\n')
print(json.dumps(results))
