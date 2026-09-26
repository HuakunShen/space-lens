/** Dedicated static pack entry; no SvelteKit routes or standalone transports. */
import { mount } from 'svelte'
import XrossPage from './routes/xross/+page.svelte'
import './app.css'

mount(XrossPage, { target: document.getElementById('app')! })
