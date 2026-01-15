import './index.css'
import { createRoot } from 'react-dom/client'
import { createElement } from 'react'
import App from './App'

const root = createRoot(document.getElementById('app'))
root.render(createElement(App))

