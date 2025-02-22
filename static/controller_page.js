document
  .getElementById("btn-unlink-discord")
  ?.addEventListener("click", (e) => {
    e.preventDefault();
    const cid = e.target.closest("button").getAttribute("controller-cid");
    const result = window.confirm(
      "Are you sure you want to unlink this controller's Discord account?"
    );
    if (result) {
      fetch(`/controller/${cid}/discord/unlink`, {
        method: "POST",
      })
        .then(() => {
          window.location.reload();
        })
        .catch((error) => {
          console.error(error);
          window.alert(`Something went wrong: ${error}`);
        });
    }
  });

document
  .getElementById("btn-modal-set-ois-close")
  .addEventListener("click", (e) => {
    e.preventDefault();
    document.getElementById("modalChangeOI").close();
  });

document
  .getElementById("btn-modal-set-roles-close")
  .addEventListener("click", (e) => {
    e.preventDefault();
    document.getElementById("modalChangeRoles").close();
  });

document
  .getElementById("btn-modal-certs-close")
  .addEventListener("click", (e) => {
    e.preventDefault();
    document.getElementById("modalCertifications").close();
  });

document
  .getElementById("btn-modal-solo-certs-edit")
  .addEventListener("click", (e) => {
    e.preventDefault();
    document.getElementById("modalSoloCertEdit").close();
  });

document
  .getElementById("btn-modal-solo-certs-close")
  .addEventListener("click", (e) => {
    e.preventDefault();
    document.getElementById("modalSoloCerts").close();
  });

document
  .getElementById("btn-modal-note-close")
  .addEventListener("click", (e) => {
    e.preventDefault();
    document.getElementById("modalNewStaffNote").close();
  });

document
  .getElementById("btn-modal-remove-close")
  .addEventListener("click", (e) => {
    e.preventDefault();
    document.getElementById("modalRemoveController").close();
  });

document
  .getElementById("btn-modal-loa-close")
  .addEventListener("click", (e) => {
    e.preventDefault();
    document.getElementById("modalUpdateLOA").close();
  });

document
  .getElementById("btn-modal-training-record-close")
  .addEventListener("click", (e) => {
    e.preventDefault();
    document.getElementById("modalNewTrainingRecord").close();
  });

document
  .getElementById("modalChangeOI")
  .querySelector('input[type="text"]')
  .addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      document.getElementById("modalChangeOI").querySelector("form").submit();
    }
  });

document.querySelectorAll(".btn-delete-comment").forEach((button) => {
  button.addEventListener("click", (e) => {
    e.preventDefault();
    const cid = e.target.closest("button").getAttribute("controller-cid");
    const noteId = button.getAttribute("note-id");
    const result = window.confirm(
      "Are you sure you want to delete your comment?"
    );
    if (result) {
      fetch(`/controller/${cid}/note/${noteId}`, {
        method: "DELETE",
      })
        .then(() => {
          window.location.reload();
        })
        .catch((error) => {
          console.error(error);
          window.alert(`Something went wrong: ${error}`);
        });
    }
  });
});

document.querySelectorAll(".button-delete-solo-cert").forEach((button) => {
  button.addEventListener("click", (e) => {
    e.preventDefault();
    const cid = e.target.closest("button").getAttribute("controller-cid");
    const soloCertId = button.getAttribute("solo-cert-id");
    const result = window.confirm(
      "Are you sure you want to delete this solo cert?"
    );
    if (result) {
      fetch(`/controller/${cid}/certs/solo/${soloCertId}`, {
        method: "DELETE",
      })
        .then(() => {
          window.location.reload();
        })
        .catch((error) => {
          console.error(error);
          widow.alert(`Something went wrong: ${error}`);
        });
    }
  });
});

document.querySelectorAll('.button-edit-solo-cert').forEach((button) => {
  button.addEventListener("click", (e) => {
    e.preventDefault();
    const soloCertId = button.getAttribute("solo-cert-id");
    const position = e.target.closest("li").querySelector("strong").innerText;
    document.getElementById("modalSoloCerts").close();
    document.getElementById("solo-cert-edit-position").innerText = position;
    document.getElementById("solo_cert_id").value = soloCertId;
    document.getElementById("modalSoloCertEdit").showModal();
  });
});

document.getElementById("input-timezone").value =
  Intl.DateTimeFormat().resolvedOptions().timeZone;
