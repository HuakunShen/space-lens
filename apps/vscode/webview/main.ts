import '@space-lens/web-ui/styles.css'
import App from './App.svelte'
import { mount } from 'svelte'

const target = document.getElementById('app')
if (target !== null) {
  mount(App, { target })
}
