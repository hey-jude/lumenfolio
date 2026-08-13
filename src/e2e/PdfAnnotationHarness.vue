<script setup>
import { ref } from 'vue'
import PdfAnnotationViewer from '../components/PdfAnnotationViewer.vue'
import { messages } from '../i18n.js'

const document = ref({
  id: 'e2e-pdf-annotation',
  title: 'annotation-fixture.pdf',
  shortTitle: 'annotation-fixture.pdf',
})

function switchDocument() {
  document.value = {
    id: 'e2e-pdf-annotation-next',
    title: 'annotation-fixture-next.pdf',
    shortTitle: 'annotation-fixture-next.pdf',
  }
}
</script>

<template>
  <main class="pdf-annotation-harness">
    <button class="switch-document" type="button" @click="switchDocument">Switch document</button>
    <PdfAnnotationViewer
      :document="document"
      :active-page="1"
      :ui="messages.en"
      target="source"
    />
  </main>
</template>

<style scoped>
.pdf-annotation-harness {
  display: flex;
  width: 100vw;
  height: 100vh;
  overflow: hidden;
}

.switch-document {
  position: fixed;
  right: 12px;
  bottom: 12px;
  z-index: 100;
}
</style>
